# live-frontend: a frontend that is a typed network of cells (design, 2026-11-16)

A page that is not one program. Every panel is a UI cell with its own backend cell, its own typed contract, its own
node and its own version. The page starts empty. Cells announce themselves, the viewer's capability chain decides
which of them the viewer is ever sent, and a cell can be replaced, replicated, refused or revoked while the rest of
the page keeps running. Nothing is recompiled to add a feature: features are discovered.

Public repo `ce-net/live-frontend`, Apache-2.0. Public text says "live system". Light theme by default.

## 0. What exists and is reused

| Need | Reused | Where | How it gets into a fresh clone |
|---|---|---|---|
| The `ui { }` half of a contract: props, needs, events, data, streams, slots, speaks, `Part<T, Right>` | the contract text of `views!` | `repo:ui-cells/crates/ui-part` | same text, compiled here by `lf-contract` (section 3.1) because `ui-part` links `ce-wire` |
| View runtime, page boot, tokens and components | `ui-rt.ts`, `ui-host.ts`, `ts/kit` | `repo:ui-cells/ts` | copied to `web/vendor/ui-cells/` at a pinned commit; `xtask vendor --check` compares sha256 in CI |
| Manifest resolve per viewer, `may_fetch` | the rules of `ui-manifest` | `repo:ui-cells/crates/ui-manifest` | same rules in `fe-manifest`, with a test that runs both on the same inputs when `ui-cells` builds (optional feature `parity`) |
| Contract macro without private crates | `wire-lite` (same api name, op ids, rights, schema hash as `ce_wire::api!`) | `repo:live-refs/crates/wire-lite` | git dependency pinned by rev |
| Nodes as processes, kill, partition, heal | `sim-mesh` | `repo:live-refs/crates/sim-mesh` | git dependency pinned by rev |
| Signed, hash-chained steps and their verifier | `live-proof` | `repo:live-refs/crates/live-proof` | git dependency pinned by rev |
| Pinned references | `ce-ref-id` | `repo:ce-ref-id` | git dependency pinned by rev |
| Chains: verify, narrow, caveats | cap v3 shape; the slice in `shop-chain` | `repo:live-shop/crates/shop-chain` | `fe-chain` starts from that code, adds `count`, `rate`, `holder` |
| Row filter from caveats | `layer_view` rules | `repo:ui-cells/crates/ui-chain` | same rules in `fe-chain::layer_view` |
| Lint of UI sources | `decl.lint` rules (FULLSTACK-CELLS 4.4) | `repo:ce/docs/FULLSTACK-CELLS.md` | `lf check` implements the four rules |
| Page tokens, card components, index | `tokens.css`, `components.css`, `demos.json` | `repo:demos` | copied with a hash check |
| One approval for what only the owner can grant | `grant.bundle@1` | `repo:grant-bundle` | used only by live mode (section 7) |

Rule (the live-shop approach): the repo builds from a fresh clone with only public dependencies. The private
`ce-wire` and `ce-gate` are replaced by `wire-lite` and `fe-gate`, which keep the same contract text, op ids, right
names and refusal shape, so each cell moves to the real crates by changing one dependency line.

Not verified at design time: that `ce-net/live-refs` is public at the pinned rev (if not, the three crates are
carried in `vendor/` with their licence and the README says so in its first paragraph).

## 1. Rules

1. One cell, one contract, one crate pair (`cells/<name>/api`, `cells/<name>/cell`) and one UI directory
   (`cells/<name>/ui`). No cell links another cell's code. A UI cell calls only its own backend.
2. The shell has no list of cells. Removing every cell leaves an empty page that still loads.
3. A right is checked on the node, never in the browser. The shell has no function that tells a view which rights
   the viewer holds; a view can only observe that a slot or a `Part` is absent.
4. A bundle runs only after its sha256 matches the signed manifest entry.
5. Every claim on the page is a step in a signed log that the viewer can verify in the browser.
6. The page always says which mode it is in: sim in this browser, replay of a recorded run, or live nodes.

## 2. Cells

Four nodes: `a`, `b`, `c` serve cells; `d` is the edge (the door the browser reaches). In sim they are processes
(native) or Web Workers (browser). In live mode they are real nodes, named on the page by class and key
fingerprint only.

| UI cell | Backend cell | Nodes | Needs | Shows |
|---|---|---|---|---|
| `systems.card` (one instance per live system) | `systems.catalog@1` | a, b | `systems.catalog:read` | section 6: the index of live systems, each card its own cell |
| `notes.board` | `notes@1.0`, then `1.1`, then `2.0` beside `1` | a, b, c | `notes:read`, `notes:write` (visitor: only rows where `sandbox == <visitor>`) | hot swap, state carry-over, major negotiation |
| `status.panel` | `status@1` | a, b, c (3 replicas) | `status:read` | failover without reload |
| `trend.chart` | `trend@1` | c | `trend:read` | composition: embedded in `notes.board` and `status.panel` through a typed slot |
| `timeline.view` | `fe.history@1` | d | `fe.history:read` | section 5: the system changing over time, across nodes |
| `cap.inspect` | `fe.explain@1` | d | none beyond a valid chain (a chain may ask about itself) | the viewer's chain, per cell: issuer, links, rights, expiry, uses left, why a cell is missing |
| `faults.panel` | `faults@1` | b | `faults:run` (sandboxed: acts on this viewer's session only) | a UI cell that crashes on request; isolation and restart |
| `ops.panel` | `ops@1` | a | `ops:read` (no visitor chain holds it) | never sent: not in the manifest, bundle fetch refused |

Runtime cells (no panel): `fe.manifest@1`, `fe.serve@1`, `fe.visitor@1`, `fe.door@1`.

## 3. Contracts

### 3.1 One contract, both halves

`lf-contract::api!` takes the `ce_wire::api!` text plus the `ui { }` block of FULLSTACK-CELLS section 2. It expands
to `wire-lite`'s api (types, op ids, rights, schema hash) and to the view table. From the same tokens `lf gen`
writes `cells/<name>/ui/gen/` (typed client, props, events, slot types). The build fails when a view needs less
than its data ops need, or names an op, event or slot type that does not exist.

```rust
api! { notes @ 1.1;
  types {
    Note   { id: NoteId, text: String, #[since(1.1)] colour: Option<Colour>, sandbox: VisitorId }
    Draft  { text: String, #[since(1.1)] colour: Option<Colour> }        // the view's own state
  }
  ops {
    1 List(())        -> Vec<Note>        needs "notes:read";
    2 Put(Note)       -> Note             needs "notes:write";
    3 Watch(())       -> stream Delta     needs "notes:read";
    9 Purge(())       -> u32              needs "notes:admin";            // no visitor chain holds this
  }
  ui {
    view notes.board { needs "notes:read"; data List, Put; streams Watch;
      state Draft;                                   // carried across a hot swap
      slot trend: trend.chart { props TrendProps };  // typed slot, filled by another cell
      speaks NoteAdded { id: NoteId };               // the only thing other cells can hear
    }
  }
}
```

### 3.2 Runtime contracts

| Cell | Ops (right) |
|---|---|
| `fe.manifest@1` | `Announce{entry, sig}` (`fe.manifest:announce`, held by serving cells), `Resolve{} -> Vec<Entry>` and `Watch{} -> stream Appeared \| Changed \| Gone` (any valid chain; answers are filtered by that chain), `Report{entry, holder, why}` (any valid chain; rate caveat) |
| `fe.serve@1` | `Fetch{sha256} -> bytes \| Refused{need}` (`may_fetch`: the caller's chain must satisfy the `needs` of a view whose entry names that hash) |
| `fe.visitor@1` | `Join{holder_pub, pow} -> Chain`, `Revoke{chain_id}` (the holder of that chain, or `fe.visitor:admin`), `State{chain_id} -> {uses_left, until, revoked}` |
| `fe.explain@1` | `Why{target: View \| Op \| Part} -> Verdict{held, missing, failed_caveat}` about the caller's own chain |
| `fe.history@1` | `Since{seq} -> Vec<Event>`, `Watch{}`; append is `fe.history:append`, held by `fe.manifest`, `fe.serve`, `fe.visitor` and the gates |
| `fe.door@1` | the one socket the browser has; forwards frames, adds nothing, holds no right of its own |

`Entry { view, instance, api: "notes@1.1", schema: sha256, bundle: ce-ref (repo:live-frontend/cells/notes/ui@<commit>
sha256:<hash>), needs, slots, speaks, state_schema, holders: [node], lease_until, sig }`, signed by the node key of
the announcing cell. An entry lives five seconds unless renewed; a dead cell disappears from the page by itself.

## 4. Runtime in the browser

### 4.1 Shell and loader (`web/shell`, on the vendored `ui-host.ts`)

1. Open one socket to `fe.door`. Obtain a chain (section 7). `Watch` the manifest with it.
2. For each `Appeared`: fetch the bundle by hash from any holder, compute sha256 with WebCrypto, compare with the
   signed entry. Mismatch: refuse, `Report` the holder, try the next holder, write the step.
3. Mount the cell in its sandbox and add its tab or panel. `Gone`: unmount, keep its state snapshot for 60 s.
4. `Changed` (new bundle or contract for one view): section 4.4. No other cell is touched.

### 4.2 Sandbox: one realm per cell, one bus

Each UI cell runs in its own `<iframe sandbox="allow-scripts">` with `srcdoc`, so it has an opaque origin: no
access to the shell's DOM, storage, cookies or to another cell's frame. The frame's CSP is
`default-src 'none'; script-src 'sha256-<bundle>'; style-src 'sha256-<kit>'; img-src data:`: the browser itself
refuses to run a changed bundle, and the cell has no network at all. Shadow DOM was rejected: it isolates styles,
not data or crashes.

The cell's only link to the world is one `MessageChannel` port to the shell's bus. The bus holds the cell's
manifest entry and allows exactly:

| Message from a cell | Allowed when | Otherwise |
|---|---|---|
| `call{op, body}` | `op` is in the entry's `data` or `streams`, body decodes against the schema hash | `NotDeclared` (never leaves the browser) |
| `emit{event}` | event is in the entry's `speaks` | `NotDeclared` |
| `slot.open{name, props}` | slot is declared, props decode against the child's `props` type | `SlotTypeMismatch` |
| `state.save{bytes}` | size limit, decodes against `state_schema` | dropped |
| `beat` | every second | missing three in a row: the cell is treated as crashed |

The bus attaches the chain to an allowed call, narrowed to that cell's api (`narrow(api == notes)`), and sends it
through the socket. A cell never sees the chain or another cell's replies. An event reaches only cells whose entry
lists it under `hears`, with the same schema hash. This bus is a convenience, not the security boundary: the node's
gate checks every call again (section 7).

Crash: an uncaught error, a rejected render, or three missed beats removes the frame; the panel shows "restarting",
the shell mounts it again after 0.5 s, 1 s, 2 s (then stops and says so) and hands back the last saved state. Other
frames and the socket are not touched. Not verified: whether a tight synchronous loop inside a sandboxed frame can
stall the shell's thread in every browser (Chromium isolates sandboxed frames in their own process); the recorded
run tests it in Chromium and Firefox and the page states the result per browser.

### 4.3 Composition through typed slots

A view declares `slot trend: trend.chart { props TrendProps }`. The shell lays a panel out as the parent's frame
plus one region per declared slot; when the parent sends `slot.open`, the shell mounts the child cell (its own
frame, bundle, node, chain narrowing) in that region with the typed props. Parent and child never share a realm;
they exchange only declared events. If the viewer may not see the child, or its node is down, the region stays
empty and the parent still works.

### 4.4 Hot swap and version negotiation

| Change | What happens | What the viewer sees |
|---|---|---|
| New bundle, same contract | new entry with the same `api`, new hash | that panel re-mounts once; state restored |
| Minor (`notes@1.0 -> 1.1`): added op, added optional field | backend 1.1 is started on one node, then the others, each announcing; 1.1 answers 1.0 callers (unknown fields are kept) | panel re-mounts on 1.1; the draft being typed is still there; a colour picker has appeared |
| Major (`notes@2.0`): a field changes meaning | `notes@1` and `notes@2` are served side by side; entries carry `supersedes` and `until`; the shell mounts the highest major it has a bundle for and keeps `@1` open until `until` or until the viewer closes it | two panels for a while, labelled 1 and 2, on the same rows (migration both ways, FULLSTACK-CELLS 6.2); then 1 leaves |

Swap order for one cell: `state.save` is requested, the frame is frozen (calls queued, 2 s limit), the new bundle is
verified, the new frame is mounted with `restore(state, from: "1.0")`, queued calls are replayed, the old frame is
removed. If the new frame fails to start, the old frame is thawed and the swap is refused with a step. Proof that
others kept running: every cell carries a counter of frame boots and a continuous `beat` sequence; the step
asserts that only the swapped cell's boot counter changed and no other cell missed a beat.

### 4.5 Scale and failover

An entry lists every holder. Reads go to one holder; on socket error or a 1.5 s deadline the bus retries the next
holder inside the same call. Writes carry an op id, so a retried `Put` is applied once. Streams reconnect to
another holder from the last sequence number. Bundles come from any holder because they are addressed by hash.

## 5. One system changing over time (`timeline.view`)

`fe.history` is an append-only, hash-chained log of what happened to the system: `Announced`, `Mounted`, `Swapped`,
`Negotiated`, `Refused`, `Crashed`, `Restarted`, `Killed`, `FailedOver`, `Joined`, `Revoked`. `timeline.view` draws
it as lanes: one row per node, one mark per cell version on that node, time left to right. The viewer watches
`notes` go from 1.0 on three nodes, to 1.1 node by node, to 2.0 beside 1, while a replica of `status` dies and
comes back. In replay and sim a scrubber sets the whole page to any step. In live mode the lanes are the real
nodes' history and the scrubber is read-only.

## 6. The live systems index becomes cells (`ce-net/demos`)

The index page becomes the same shell. Each card is an instance of `systems.card`, announced by that live system
itself:

- Each live system repo publishes `ce-ui.json` (its signed entry: name, line, status, links, card bundle hash) and
  the card bundle on its own Pages site. A system that adds this file appears on the index without a commit to
  `ce-net/demos`; a system whose entry fails its signature or hash is shown as refused, with the reason.
- The index holds only `holders.json`: the origins to ask (on a static host there is no gossip, so the list of
  places is fixed; the list of cards is not). When the live path (section 7) is up, `fe.manifest` on the real nodes
  replaces `holders.json` and a card appears or leaves when its cell does.
- A card may fill its `preview` slot with a cell from its own system (the shop's stock count, this repo's lanes).
- Without JavaScript the page shows a plain list generated in CI from the same entries, and `demos.json` stays as
  a generated file for existing readers.
- `live-frontend` is the headline card.

## 7. Real capabilities on real identities

### 7.1 The visitor chain

```
ce-net site root (keys cell; never leaves it)
 └─ fe.visitor issuer      fe visitor rights, mint <= 500 chains/day, until <= +90 days      (granted once, 7.3)
     └─ visitor <fingerprint>   holder == this browser's key, until +15 min, count 40, rate 2/s
          ├─ systems.catalog:read, status:read, trend:read, fe.history:read
          ├─ notes:read, notes:write   where sandbox == <fingerprint>   (max 20 rows, 200 bytes each)
          └─ faults:run                where session == <fingerprint>
```

- The browser creates a non-extractable Ed25519 key (WebCrypto) and calls `fe.visitor Join`. `Join` is limited per
  network address (3 per hour) and asks for a small proof of work. Every call is signed by the holder key, so a
  copied chain is useless.
- The gate on each real node (`fe-gate`, the `ce-gate` rules: op right, expiry, holder, `where`, plus the counted
  right backed by one counter row per chain id) checks the full chain on every call. Two real nodes of different
  classes (one cloud node, one desk node) run the cells, so the page shows the same chain accepted by two gates.
- Visitor writes go to a sandbox table that only that visitor reads, and are deleted when the chain ends.
- No owner, operator or agent right is in the chain or reachable from it. `ops:read`, `notes:admin`,
  `fe.visitor:admin` and every `:announce` right are outside the issuer's grant, so the issuer cannot mint them.

### 7.2 What the page shows

- `cap.inspect`: the actual chain. Issuer fingerprint, each link, rights, caveats, expiry as a countdown, uses
  left (from `fe.visitor State`, decreasing as panels load), and under each cell on the page the rights that let
  it appear and the node whose gate last checked them.
- "Allowed": `notes.Put` in the visitor's sandbox, with the gate's node, the right matched and the time.
- "Refused": a button calls `notes.Purge`; the gate's own refusal text is printed unchanged
  (`Refused{need: "notes:admin", chain: <id>, node: <fingerprint>}`). A second button spends the remaining count
  and shows the count refusal.
- "Revoke my chain": `fe.visitor Revoke`; the gates refuse the next call, `fe.manifest Watch` sends `Gone` for
  every entry, and the panels leave the page one by one without a reload. Expiry does the same.
- Every one of these is appended to `fe.history` by the gate that decided it, so it is on the timeline.

### 7.3 What only the owner can grant (one `grant.bundle` card)

| Item | Scope | End |
|---|---|---|
| Issuer grant for `fe.visitor` under the ce-net root | exactly the rights of 7.1, mint limit, no delegation beyond one link | 90 days |
| Install of the cells of section 2 on one cloud node and one desk node | `cell.install` for those ce-refs at pinned hashes | once |
| A public route for `fe.door` (`wss` on a ce-net host name) | forwards only the apis of section 2 | 90 days |

Until the card is approved the page runs in sim and replay, and says so. When `live.json` (door address, node
fingerprints, root public key) is present and the door answers, a "Live nodes" mode appears; nothing else changes,
because sim and live use the same shell, bundles and contracts. Not verified at design time: whether the public
door's `agent_join` already gives a browser a narrow chain; if it does, `fe.visitor Join` calls it instead of
minting.

Public page rules: nodes appear as class and key fingerprint, never as host names; no token is written to storage,
a URL or the log (the log carries chain ids and hashes).

## 8. Sim, recorded run, proof

One backend core per cell: `fn handle(op, body, chain, store) -> reply`, with no I/O of its own.

| Mode | Nodes | Transport | Command or place |
|---|---|---|---|
| `sim` (native) | 4 processes on loopback (`sim-mesh`), own keys | TCP + one WebSocket door | `cargo cell sim` |
| `record` | as `sim`, plus a headless Chromium and Firefox opening the real page | same | `cargo cell record` writes `proof/run.jsonl`, `proof/roster.json`, screenshots |
| browser sim (Pages) | 4 Web Workers, each the same backend cores compiled to `wasm32`, each with its own key | `postMessage` behind the same door interface | <https://ce-net.github.io/live-frontend/> |
| replay (Pages) | the browser sim, driven by the script and seed of `proof/run.jsonl` | same | "Replay recorded run"; each reply's sha256 is compared with the recorded one and ticked or flagged |
| `live` | real nodes | `wss` door | `cargo cell record --live`, writes `proof/live-<date>/` |

The recorded run is one script, `proof/script.toml`, and each line is asserted and written as a `live-proof` step
(`n, cell, op, node, request_ref, response_sha256, ms, verdict`), signed by the node that answered:

| Step | Action | Assertion |
|---|---|---|
| P1 | open the page with no chain | zero frames, zero bundles fetched |
| P2 | join as a visitor | cards and panels appear in announce order; `ops.panel` absent from `Resolve`; direct `Fetch` of its hash is refused |
| P3 | start `faults` backend on b while the page is open | its tab appears; no reload, no rebuild |
| P4 | deploy `notes@1.1` node by node | only `notes.board` boots again; draft text equal before and after; no other cell misses a beat |
| P5 | deploy `notes@2.0` | panels 1 and 2 side by side on the same rows; 1 leaves at `until` |
| P6 | node c serves a bundle with one changed byte | refused with want and got; loaded from another holder; holder reported |
| P7 | `notes.board` tries `status.Read`, a `fetch`, and `parent.document` | `NotDeclared`; blocked by CSP; security error |
| P8 | `faults.panel` throws, then stops answering | frame restarted with state; all other boot counters unchanged |
| P9 | kill node b while `status.panel` streams | stream continues from the next sequence number on another holder; no reload; `Put` with a retried op id applied once |
| P10 | `notes.board` opens its `trend` slot | child frame from node c; wrong props type is refused at build (`lf check`) and at the bus |
| P11 | allowed call, refused call, count exhausted | gate texts as in 7.2 |
| P12 | revoke the chain | next call refused; every panel leaves; zero frames |

`cargo cell verify` checks the signatures, the hash chain and the roster; the page does the same check in the
browser and shows the result next to the run.

## 9. Developer loop

`.cargo/config.toml` holds `alias cell = "run -q -p lf --"`, so:

| Command | Does |
|---|---|
| `cargo cell dev` | starts every cell under `cells/` on four local nodes and serves the shell; watches each cell's directory; a change rebuilds only that cell (backend, or UI bundle, or both), announces the new entry, and the open page swaps that one panel with its state |
| `cargo cell check` | contract-break check across cells: each `api` is compared with `contracts/frozen/<api>@<major>.json`; a removed or retyped op, field, event, slot prop or right is an error that names every cell and view depending on it; an addition without a minor bump is an error; then the four `decl.lint` rules on `cells/*/ui/src` |
| `cargo cell freeze <api>` | writes the frozen descriptor for a released version |
| `cargo cell new <name>` | a new cell pair and UI directory from a template; it appears on the open page when it first builds |
| `cargo cell sim \| record \| verify` | section 8 |

## 10. Repository

```
cells/<name>/{api,cell,ui}      the eight cell pairs of section 2
crates/lf-contract              api! with ui { } over wire-lite
crates/fe-chain  fe-gate        chains, caveats, counted rights, layer_view; the gate and its refusal text
crates/fe-manifest fe-serve fe-visitor fe-explain fe-history fe-door
crates/lf                       the `cargo cell` tool: dev, check, freeze, new, gen, sim, record, verify
crates/sim-wasm                 the backend cores as one wasm module per node role
web/shell                       loader, sandbox, bus, slots, swap, failover (on vendored ui-host.ts, ui-rt.ts, kit)
web/page                        the Pages page: nodes, shell, timeline, proof, controls
contracts/frozen  proof/  docs/DESIGN.md  README.md  .github/workflows/ci.yml
```

Needs Rust stable (with the `wasm32-unknown-unknown` target) and Node 20. Quickstart:

```sh
git clone https://github.com/ce-net/live-frontend && cd live-frontend
npm ci --prefix web
cargo cell dev            # open http://127.0.0.1:21200/
```

CI: fresh clone, `cargo cell check`, `cargo test --workspace`, `cargo cell record` (Chromium and Firefox),
`cargo cell verify`, vendor hash check, wasm build, Pages deploy of `web/page` with `proof/`.

## 11. The page

One screen, light theme, the tokens and components of `repo:demos`.

- Top bar: name, mode (Sim in this browser / Replay of the run recorded <date> / Live nodes), the chain chip
  (fingerprint, uses left, time left).
- Left: the four nodes with the cells and versions on each, alive or stopped.
- Centre: the page being assembled: empty at first, then tabs and panels as cells are discovered.
- Right: `cap.inspect`.
- Bottom: the timeline lanes with the scrubber, and the proof steps P1 to P12 with their verdicts.
- Controls, each one a real action on the sim nodes: Join, Start a cell, Deploy notes 1.1, Deploy notes 2.0,
  Change one byte of a bundle, Crash a cell, Stop a node, Call something I may not, Revoke my chain, Replay
  recorded run.
- At phone width the columns stack: page, controls, chain, nodes, timeline.

## 12. Slices

1. Contracts, chains, gate, manifest, serve, shell with sandbox and bus, `notes` and `status`; native sim; P1 to
   P4, P6, P7, P9, P12 recorded; README quickstart; CI.
2. `trend` slots, `faults`, major negotiation, `timeline.view`, `cap.inspect`; P5, P8, P10, P11; wasm browser sim
   and replay on Pages.
3. `systems.card`, `ce-ui.json` in each live system repo, the index of `ce-net/demos` on the shell, headline card.
4. Live mode: the `grant.bundle` card of 7.3, cells on the two real nodes, `proof/live-<date>/`.

## 13. Open points

- `ce-net/live-refs` public at the pinned rev (section 0); otherwise `vendor/`.
- Frame isolation against a tight loop per browser (4.2); measured in `record`, stated on the page.
- `agent_join` on the public door (7.3).
- A static host cannot withhold a file: `ops.panel`'s bundle is not published on Pages at all, and "never sent" is
  shown there by the sim nodes and proven on real nodes only in live mode.
- Determinism of the browser replay needs a seeded clock and seeded keys in the sim nodes; live steps are verified
  by signature only, not re-executed.
