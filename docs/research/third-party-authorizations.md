# Third-party authorizations: one model for a thousand tools

Every developer brings different tools — Artifactory, Sentry, Elastic,
their company's own — and each wants a credential. Wormhole cannot know
them, and must not try: a per-tool integration list is a treadmill that
is wrong the day it ships. What scales is a small set of *generic*
mechanisms that every tool falls into. There are exactly three, and two
already exist.

## What "a tool's auth" actually is

Strip any tool's authentication to its parts and only two remain: some
secret bytes, and a host to present them to. The delivery varies —
an env variable (`SENTRY_AUTH_TOKEN`, `ARTIFACTORY_ACCESS_TOKEN`,
AWS keys), a dotfile (`~/.npmrc`, `~/.netrc`, kubeconfig), a URL the
client is pointed at — but that is packaging, not substance. So the
support matrix is not tools × wormhole; it is three delivery shapes ×
one mechanism each.

## Tier 1 — reachability: `egress` (built)

`[access] egress` names the hosts; the broker's `CONNECT` leg carries
any HTTPS client to them, resolving and dialing host-side, intercepting
nothing. This is tool-agnostic by construction: Artifactory, Sentry,
Elastic, anything self-hosted — one manifest line each, zero wormhole
code, the tool speaks its own protocol end to end.

```toml
[access]
egress = ["mycorp.jfrog.io", "sentry.io", "my-cluster.es.cloud.example"]
```

## Tier 2 — secret delivery: the store, `ask`, and grants (built)

For the overwhelming majority of tools the credential is an env
variable or a small file, and the box must hold it to use it — see
"why not always Tier 3" below.

- **Env secrets**: `[env.SENTRY_AUTH_TOKEN] ask = true`. Asked once on
  the first start anywhere, kept host-side in the secret store, filled
  into every box of every role that declares the name. Ten roles, one
  Sentry token, typed one time.
- **File secrets**: `[access] grants = ["~/.npmrc"]` — one file,
  read-only, previewed at start. Never a directory of them.

**The exposure, named**: a secret the box holds, the agent can read.
The model is honest about it and bounds the blast radius twice over:

1. **Scoped tokens, never passwords.** The practice the docs teach: a
   Sentry auth token scoped to one project, an Artifactory reference
   token, an Elastic API key with one role. Revocable at the service,
   useless beyond its scope.
2. **Egress bounds exfiltration.** A stolen token is only useful if it
   can leave, and the only places bytes can go are the allowlisted
   hosts. A box that reaches `sentry.io` and `crates.io` cannot mail
   your token anywhere else — there is no route.

The pair is the security floor for arbitrary tools, and it is the same
floor Vault agents and CI secret scopes settle on.

## Tier 3 — broker-held: the secret never enters the box

The strongest tier, and the one that cannot be universal. Injecting a
credential without TLS interception requires the *client* to be
pointed at the broker as its origin (`ANTHROPIC_BASE_URL` today). Many
tools allow exactly that — package registries (npm, pip, cargo all take
a registry URL), Elastic (endpoint URL), Artifactory (registry/API
URL) — and for those a **data-driven injected route** generalizes the
model-API leg with zero per-tool code:

```toml
[[access.service]]
name = "artifactory"                  # box env: ARTIFACTORY_URL -> forwarder
upstream = "https://mycorp.jfrog.io"
header = "Authorization"              # value: "Bearer " + secret
secret = "ARTIFACTORY_TOKEN"          # from the host-side store
```

Broker reverse-proxies the route, injects the header from the secret
store, streams back. The box sees a loopback URL and no secret. Not
built yet; the design is this table and the existing model-API relay
with the header set made data. Build it when the first real role wants
it — the manifest shape above is the contract.

**Why not always Tier 3**: a tool with no base-URL setting (a CLI with
a hardcoded SaaS endpoint) can only be re-routed by intercepting TLS,
and wormhole never does that (CONCEPT §2): a wormhole CA in every box
breaks pinning, adds a decrypting middlebox, and turns one bug into
every credential at once. For those tools Tier 2 is the honest
ceiling, and a scoped token behind a narrow egress list is what it
costs.

## Special cases, and why they stay special

- **Anthropic OAuth (the subscription)**: broker-mandatory — the
  refresh token rotates on every use (spike #15), so any in-box copy
  breaks the host login. This is the one protocol wormhole terminates
  with bespoke code, because correctness demands it.
- **Git/GitHub**: planned fetch-only git broker with token injection
  and `GET`-only API (CONCEPT §2) — bespoke because the *policy*
  (read-anything-change-nothing) needs protocol awareness, not because
  auth does. Public repos already work through Tier 1.

## How thousands of developers compose with this

A role declares **names, never values**: its egress hosts, its
`ask` variables, its injected routes. Roles travel through git and are
previewed and approved. Each developer's secret store fills those names
locally, once per secret, shared across all their roles. The same role
serves everyone; nobody's token is in it; wormhole ships no
integration for any of the thousand tools — and needs none.

| Shape | Mechanism | Per-tool wormhole code |
|---|---|---|
| Reach a host | `egress` + CONNECT | none |
| Env secret | `ask` + secret store | none |
| File secret | single-file grant | none |
| Base-URL client | `[[access.service]]` injected route (design) | none |
| Rotating OAuth (Claude) | model-API broker leg | the one exception |
| Git write-policy | git broker (planned) | policy, not auth |
