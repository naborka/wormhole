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
an untrusted agent talks to. It is a small static helper carried inside
wormhole's own binary and written into the box under `/run` at start —
nothing to install. Static against musl, so it runs in any image no matter
which C library the image ships, and no matter how wormhole itself was
built.

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

## The `CONNECT` leg

The same socket carries everything else the box may reach. Tools that
honor `HTTPS_PROXY` — `cargo`, `git`, `npm`, `curl`, and wormhole sets
it for every brokered box — send `CONNECT host:443`; the broker judges
the host against the manifest's `egress` allowlist, resolves and dials
it host-side, and then moves bytes without looking inside — the TLS in
the tunnel is the client's own, nothing is intercepted, no wormhole CA
exists. The box needs no resolver for any of this.

A blocked host is a real `403` naming the host and the exact command
that unblocks it — never a silent failure or a hang. Only ports 443
and 80 are tunneled; any other port would make the tunnel a generic TCP
channel. The allowlist baseline is empty: `CONNECT` reaches nothing
that was not named.

## Changing the list while the box runs

```sh
wormhole allow api docs.rs      # effective on the very next tunnel
wormhole deny  api docs.rs      # new tunnels refused from now on
```

Nothing restarts — not the box, not the broker. The policy never lived
inside the box: the broker reads the box's live allowlist file per
tunnel, so an `allow` typed on the host is already in force when the
agent retries. The agent's own `403` says the line to type. An `allow`
is also kept in the box's home, so the same box remembers it across
restarts; the manifest stays the role's word and is never edited for
you. Only a person on the host can type it — nothing inside a box can
widen its own list. A `deny` refuses new tunnels immediately; one
already open lives until either side hangs up, the same honesty
`umount` owes to open file descriptors. Denying a host the *manifest*
names holds only until the box's next start — the manifest is the
durable word, so make a permanent removal there.

## Using it

Nothing to start. A brokered box — the default for any manifest with an
agent — spawns its own broker: this manifest's `egress` list, a socket
in the box's own directory, ended with the box. One box's broker never
serves another, so revoking one box's reach never touches its neighbour.

You do not declare `ANTHROPIC_BASE_URL`, `ANTHROPIC_API_KEY` or
`HTTPS_PROXY`: wormhole sets them, pointing the agent and every proxy-
aware tool at the forwarder and handing the agent the dummy key the
broker later strips.

The banner names what the box got:

```
boundary: namespaces (host kernel SHARED) · egress: model api + 5 allowed hosts via the broker · workspace: rw · grants: 0
```

`wormhole broker --socket PATH --egress H1,H2` still exists for running
one by hand.

## `network = "host"`

The opt-out. The credential still moves to the host when the broker is
on, but the box has every route the host has and can reach anything on
its own; `egress` is refused there, because nothing would enforce it.
The banner says all of it.

## What it costs today

| | |
|---|---|
| `curl` per request upstream | On the model-API leg only; no connection reuse. The host's CA store, proxy settings and TLS stay the host's business, which is the trade — wormhole owning a TLS stack would be a large thing to get wrong for no gain |
| Plain-HTTP proxying is not carried | `HTTPS_PROXY` covers `https://` URLs; a run-time `apk add` over plain `http` has no path. Packages belong in the image anyway |
| The account's reach is unchanged | A broker cannot see what a *credential* carries. Read [what the box can reach](access.md#what-a-credential-grant-really-carries) — hosted connectors execute server-side and no boundary here touches them |
