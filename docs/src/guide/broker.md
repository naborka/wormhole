# The broker

Every other page describes what the box cannot *see*. The broker is what
finally decides what it can *reach*.

Without it, a box holds your Claude credential file and opens its own
connection to the API. Both of those are things an agent you do not trust
now holds. The broker takes them back:

- the credential stays on the host, in one process
- the box gets a socket, not a route
- what comes back from that socket is the model API and nothing else

## The two halves

```
   box                        │  host
   ───────────────────────────┼──────────────────────────────
   agent                      │
     └─ ANTHROPIC_BASE_URL    │
        http://127.0.0.1:8787 │
          └─ forwarder ───────┼─ broker.sock ─ wormhole broker
             (moves bytes,    │                (holds the token,
              knows nothing)  │                 renews it, injects it)
                              │                     └─ api.anthropic.com
```

The **forwarder** runs inside the box on loopback and does one thing: move
bytes between a TCP connection and the unix socket. It holds no credential
and can reach nothing else, which is why it is safe for it to be the thing
an untrusted agent talks to. It is wormhole's own binary, bound read-only
into the box under `/run` — nothing to install.

For that to work inside an image, the binary must not need the host's C
library: an Alpine image has no glibc loader. Build wormhole static (see
[Install](install.md)); a dynamically linked binary is refused at launch
with the rebuild command, instead of dying inside the box with a bare
"not found".

The **broker** runs on the host. It reads `~/.claude/.credentials.json`,
strips the box's headers, injects the real token, and streams the reply
straight back.

What it strips on the way out: the dummy API key the agent was handed so
its client would start, and any `authorization`, `proxy-authorization`,
`connection` or `host` header the box tried to set. A caller inside the box
cannot choose its own identity upstream.

It renews the token five minutes before expiry rather than after a
rejection. Once reply headers are on the wire a retry is impossible, and
the agent would burn its seven retries on a 401 silently.

## Using it

Start the broker once, on the host. It stays in the foreground:

```sh
wormhole broker
# broker: listening on ~/.local/share/wormhole/broker.sock
```

Then in the manifest:

```toml
[access]
broker = true
network = "none"
```

Drop the `grants` entry for `~/.claude/.credentials.json` and drop `dns`
— the box needs neither any more. You do not declare `ANTHROPIC_BASE_URL`
or `ANTHROPIC_API_KEY`: wormhole sets both itself, pointing the agent at
the forwarder and handing it the dummy key the broker later strips.

```sh
wormhole box
```

The banner names what it got:

```
boundary: namespaces (host kernel SHARED) · egress: model api via the broker · workspace: rw · grants: 0
```

A manifest that brokers when nothing is listening is refused before the box
starts, naming the socket and the command that serves it.

## `broker = true` without `network = "none"`

Allowed, and honest about what it is: the credential moves to the host, but
the box still has every route the host has, so it can reach the API on its
own as well. The banner says both.

The pair is what makes the design's claim true rather than merely intended.
`broker` alone removes the credential; `network = "none"` removes the
alternative.

## What it costs today

| | |
|---|---|
| One connection at a time | An agent makes one request at a time, so nothing has needed more. A second client queues behind the one in flight |
| `curl` per request upstream | No connection reuse. The host's CA store, proxy settings and TLS stay the host's business, which is the trade — wormhole owning a TLS stack would be a large thing to get wrong for no gain |
| The account's reach is unchanged | A broker cannot see what a *credential* carries. Read [what the box can reach](access.md#what-a-credential-grant-really-carries) — hosted connectors execute server-side and no boundary here touches them |
