# live-frontend: a frontend made of cells

A live system on ce-net. The page is not one program. Every panel is a UI cell with its own backend cell, its own
typed contract, its own node and its own version. The page starts empty: cells announce themselves, and the
capability chain the browser holds decides which of them it is ever sent. A cell can be replaced, replicated,
refused or revoked while the rest of the page keeps running. Nothing is recompiled to add a feature.

- The page: <https://ce-net.github.io/live-frontend/>. It runs the whole system in your browser: four nodes, each
  a Web Worker running the same node code compiled to wasm, serving UI cells and their backends. The controls
  change the nodes underneath the page. Below it, the recorded run of real processes is checked in your browser,
  and "Replay" re-executes every input those processes logged.
- The index of live systems with every card a cell: <https://ce-net.github.io/live-frontend/systems.html>.
- Design: [docs/DESIGN.md](docs/DESIGN.md).

## Quickstart (fresh clone, public dependencies only)

Needs Rust stable (with `rustup target add wasm32-unknown-unknown` for the in-browser nodes) and Node 18 or newer.

```sh
git clone https://github.com/ce-net/live-frontend && cd live-frontend
npm ci --prefix web
cargo build --release --workspace
cargo cell build          # TypeScript clients from the contracts, every UI bundle, the page, the wasm node
cargo cell dev            # four node processes; open http://127.0.0.1:21200/
```

`cargo cell dev` watches `cells/`. Change `cells/notes/ui/src/board.ts` and only that cell is rebuilt and
re-announced; the open page swaps that one panel and keeps what you were typing. Change a backend under
`cells/*/cell` and the nodes restart one at a time on the new build while the page stays up.

The other commands:

```sh
cargo cell check          # every contract against contracts/frozen, slot prop types, decl.lint on the views
cargo cell break-proof    # three contract breaks that must fail the build (Rust, the check, and the TypeScript views)
cargo test --workspace --release
cargo cell record         # the recorded run below: real processes, a headless browser (LF_BROWSERS=chromium,firefox)
cargo cell verify proof   # signatures, hash chain, and every logged input re-executed to the same bytes
cargo cell new <name>     # a new cell pair and UI directory from the template
```

## What the recorded run shows

`proof/` is one run of four real node processes on loopback (each with its own Ed25519 key, every frame signed)
driven by headless Chromium; `proof/firefox/` is the same run in Firefox. Twelve steps, each asserted and signed
into `proof/run.jsonl`:

| Step | What happens | Recorded (Chromium) |
|---|---|---|
| P1 | page opened with no chain | 0 frames, 0 bundles fetched |
| P2 | join as a visitor | 9 cells mounted; `ops.panel` absent from the manifest; a direct fetch of its bundle hash refused by the gate |
| P3 | the `faults` backend starts on node b while the page is open | its panel appears, no reload |
| P4 | `notes` 1.0 to 1.1, node by node | only that panel boots again, in 73 ms; the half-typed draft survives; 8 of 8 other cells untouched, none missed a beat |
| P5 | `notes` 2.0 (a field changes meaning) beside 1 | two panels on the same rows, colour by name in 1 and as RGB in 2; panel 1 leaves at its end time |
| P6 | node c serves the bundle with one byte changed | refused (want and got differ), reported, loaded from another holder |
| P7 | a cell tries an undeclared call, a fetch, the parent page, storage | refused at the bus; no network in the frame; opaque origin |
| P8 | a cell throws, then runs a 4 s tight loop | the throw: that frame restarts alone with its saved state. The loop: see below |
| P9 | node b is stopped while `status.panel` streams from it | the next answer comes from c 490 ms after the kill, no reload; a write retried with the same op id is applied once |
| P10 | `notes.board` opens its typed `trend` slot | `trend.chart` mounts in its own frame from node c; wrong prop types refused |
| P11 | allowed write, `notes.Purge`, spending the counted uses | `Refused{need: "notes:admin", chain: …, node: …}` printed verbatim; `uses exhausted (40 of 40 spent)` |
| P12 | the visitor revokes its chain | the next call is refused by the gate; every panel leaves the page |

`cargo cell verify proof` re-creates the four nodes from the run's seed and re-executes every input each process
logged (thousands of frames, ticks and replies): all outputs are byte-identical. The page does the same in the
browser with the wasm build.

A tight loop inside a cell (P8) is measured, not hidden. In Chromium the shell keeps running (its thread stalled
3 ms) while the cell frames, which share renderer processes, pause for the 4.5 s of the loop. In Firefox a
sandboxed frame runs on the page's own thread, so the whole page pauses for the loop. A thrown error is isolated
in both.

## How it is built

| Part | Where |
|---|---|
| One contract, both halves: `lf_contract::contract!` takes the `ce_wire::api!` text (compiled by [wire-lite](https://github.com/ce-net/live-refs/tree/main/crates/wire-lite), same api name, op ids, rights, schema hash) plus a `ui { }` block (view, needs, data, tries, streams, props, state, slots, events). The build fails when a view reads an op whose right it does not declare. | `crates/lf-contract` |
| Chains: root, issuer, visitor; expiry, holder, counted uses, rate, a row caveat (`sandbox == <visitor>`) | `crates/fe-chain` |
| The gate on every node: chain, revocation, holder signature over the exact call, replay, right, rate, issuer-signed spend tickets; one refusal shape | `crates/fe-gate` |
| Runtime cells: `fe.manifest` (signed entries, 5 s lease, answered per viewer), `fe.serve` (bundles by hash, `may_fetch`), `fe.visitor` (issuer), `fe.history` (hash-chained log), `fe.explain`, `fe.door` | `crates/fe-runtime`, `crates/fe-host` |
| One node as a pure state machine (input to output, no clock or I/O inside) | `crates/fe-host` |
| The node as a process behind a [sim-mesh](https://github.com/ce-net/live-refs/tree/main/crates/sim-mesh) door, and in the browser as wasm | `crates/fe-node`, `crates/sim-wasm` |
| UI cells and backends | `cells/{notes,status,trend,faults,ops,systems}/{api,cell,ui}`, `cells/{timeline,inspect}/ui` |
| The shell: one `<iframe sandbox="allow-scripts">` per cell, CSP pinned to the bundle's sha256 and no network, one MessagePort per cell; the bus lets through only what the signed entry declares; swap with state, crash restart, holder failover, typed slots | `web/shell` |
| `cargo cell` | `crates/lf` |

Pinned open dependencies: `wire-lite`, `sim-mesh`, `live-proof`, `ce-ref-id` from
[ce-net/live-refs](https://github.com/ce-net/live-refs) at a fixed rev; the view runtime and kit of
[ce-net/ui-cells](https://github.com/ce-net/ui-cells) vendored in `web/vendor` with sha256 pins
(`cargo cell vendor-check`).

## Real capabilities

The visitor chain is real in every mode: the browser makes a non-extractable Ed25519 key with WebCrypto, gets a
chain root to issuer to that key (15 minutes, 40 counted uses, a rate, writes only to its own sandbox rows), and
signs every call; each node's gate checks it. In the sim, the root and node keys are derived from the run's seed.
`cargo cell live` runs the four nodes as a long-running service on a cloud node, with every key (root, issuer,
nodes) drawn from the OS in memory and never written anywhere; the config files hold public keys only.
[proof/live-2026-11-16/](proof/live-2026-11-16/) is headless Chromium on that node using a real visitor chain on
those gates: join (root, issuer, this browser's WebCrypto key), `ops.panel` absent and its bundle refused, a write
allowed, `notes.Purge` refused verbatim, the 40 uses running out, revocation emptying the page; 4 of 4 steps pass,
checked by signature (live inputs are not re-executed, since their keys are gone).

Not open yet: the issuer under the ce-net site root, the same chain checked by a second (desk) node's gate, and a
public route to the live door, so the Pages site can offer "Live nodes" next to the in-browser sim
([docs/DESIGN.md](docs/DESIGN.md) section 7.3).

## License

Apache-2.0. The vendored fonts are under the OFL (`web/vendor/ui-cells/kit/fonts/OFL.txt`).
