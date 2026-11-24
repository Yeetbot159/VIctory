# VIctory — the Yeetbot159 fork

Runs Grand Theft Auto VI (Xbox Series X|S) natively on Windows, from files the
user supplies.

The title's x86-64 code runs unchanged inside our process; VIctory supplies the
platform underneath it and converts the title's data, ahead of time where it can
and on first use where it must (shaders). See [docs/architecture.md](docs/architecture.md).

**This repository contains no game files, code or assets.** You need your own
copy of the game. Nothing here is affiliated with or endorsed by Rockstar Games
or Take-Two Interactive, and nothing in the repository is derived from the
title's content: `docs/asset-policy.md` is the rule, `cargo xtask policy` is the
check, and `.githooks/pre-commit` runs that check on every commit once
`cargo xtask install-hooks` has pointed git at it.

## The three repositories this stands on

| Repository | What it is | Licence |
|---|---|---|
| [Marvisak/VIctory](https://github.com/Marvisak/VIctory) (`origin`) | upstream: the loader, runtime services, Direct3D 12 back end and shader recompiler | GPL-3.0-or-later |
| [fxceless/victory-gta6](https://github.com/fxceless/victory-gta6) (`fxceless`) | the fork this one now follows as its base: the rendering and submission work that gets the world to draw | GPL-3.0-or-later |
| this repository (`fork`, Yeetbot159/VIctory) | one person's fork of the second, carrying its own fixes and instruments | GPL-3.0-or-later |

All three share upstream commit `ffe310e` as their common ancestor, so their
histories merge rather than being grafted: this fork's commit log contains
fxceless's own commits and upstream's, unsquashed. See
[Sources and attribution](#sources-and-attribution) for what came from where.

## Status (2026-11-24)

**Not playable.** The menus work; a rendered world frame has never come out of
this tree. The base this fork adopted reports a cutscene and a prologue on an
NVIDIA card, which is why it was adopted; those numbers are the base's, on the
base's machine, and this tree has not reproduced them yet.

| Stage | State | Produced by |
|---|---|---|
| Boot, first-boot screens, start screen, menus | works at the title's own 30 fps | this PC's card (AMD RX 6700 XT), upstream code, in the menus |
| The opening cutscene window (frames 1,280–1,300) | the presented image changes frame to frame (24–144 distinct values in a 64x64 downscale, means 105,84,100 → 63,53,64 → 72,60,70 → 33,32,44 → 44,40,53, with 1,295 nearly flat); the 3840x2160 `f19` depth target reads **one unique value, zero**, at all five | WARP, diagnostics build, keyed pad-A soak, offscreen — **not** a rendered 3D frame |
| The world, first frame | every run on this PC ends here: access violation at `victory-image-…+0x8f92a`, read of `0x17`, rax all-ones — a descriptor slot holding `-1` used as a pointer. Runs end between frames 1,250 and 1,650, or not at all | this PC's card, in the world's first frame |
| The world, the base's claim | the opening cutscene plays start to finish and the prologue renders characters, textures, sky, HUD and pause menu at **2–4 fps** | fxceless/victory-gta6, RTX 4070 12 GB / i7-14700KF, Series X profile, in the world |
| The world, this tree | **never rendered a frame** | — |
| Frame rate | 30 fps in the menus; 0.5–1.8 s per world frame | this PC's card, in the world, before the world-frame crash |
| Frame rate, the base's numbers | 420–450 ms per frame by default; 240–350 ms with `VICTORY_RELAY=2` or with `VICTORY_LATE_WAIT=3 VICTORY_WIDE_BACK=1 VICTORY_CARD_BUFFERS_MIN_MB=1 VICTORY_CARD_BUFFERS_MB=1024` | fxceless/victory-gta6, RTX 4070, in the world |

Every number here says what produced it: which tree, which adapter, and
whether the run was in the menus or in the world. Offline numbers — a
translation result, a validation count, a set of emitted DXIL — say that
instead, because no adapter touched them. A number that says none of this is a
number nobody can reproduce, and this file does not carry those.

Three statements this fork can stand behind today:

- **The cutscene window draws colour and no depth.** Five frames of a keyed
  WARP run were dumped — 1,280, 1,290, 1,292, 1,295 and 1,300 — and every
  texture each frame wrote is on disk as
  `frame-<n>-<i>-<resource>-<w>x<h>-f<format>.png`. The 3840x2160 `f19` depth
  target has exactly one unique value, zero, in all five. The colour target the
  present is copied from holds structure that changes from frame to frame: 24 to
  144 distinct values in a 64x64 downscale, with the mean sliding
  (105,84,100) → (63,53,64) → (72,60,70) → (33,32,44) → (44,40,53). Frame
  1,295's present is the exception — nearly flat at 2 distinct values while that
  frame's second colour target still holds 83, so it reads as a fade rather than
  a drawn frame. Geometry that writes colour and never depth does not exist in
  this title as far as anyone here has seen, so what is on screen is composited
  or copied content, not a rendered 3D frame.
- **No world frame has rendered in this tree.** The window above is the title's
  cutscene, and the run ends at the first world frame with the `-1` descriptor
  fault, every time, at a frame number that moves between 1,250 and 1,650.
- **The frame rate belongs to a tree and a machine.** The 30 fps above is the
  menus on an RX 6700 XT; the 2–4 fps above is the base's prologue on an RTX
  4070. Neither number describes the merged tree, which has not been measured.

### What this file did not check itself

Kept deliberately, because a claim you cannot reproduce is worth less than the
fact that you said so:

- The **545 → 0 mesh pipeline refusals with 338 modules** are the graphics lane's
  keyed soak as this crew's notes record it. Its log is not on the machine this
  was written on under any name that could be found, so it has not been
  recounted. The offline counts in the same table come from
  [docs/emit-gaps.md](docs/emit-gaps.md), which was measured.
- The **three `no host texture` failures a session** come from the review lane's
  handoff to whoever performed the merge, not from a log read here.
- The **base's** cutscene, prologue and frame-rate numbers are its own README's,
  on its own machine. Nothing in this repository reproduces them, and its report
  says so in its own words.
- The **world-frame crash** is described from this crew's crash logs on this PC,
  which are not committed because they name title addresses. The only
  `crashes.log` on disk while this was written belongs to the adoption attempt
  and carries different offsets, so the `+0x8f92a` signature is quoted from the
  crew's record rather than re-read.
- The **cutscene window's numbers** are this file's own: measured from the five
  dumped PNGs with a histogram, not quoted from anywhere.

The in-host instruments in [Debugging](#debugging) — `VICTORY_GPU_CHECK`,
`VICTORY_D3D12_VALIDATE`, `VICTORY_INDIRECT_RECORD`, `VICTORY_SERIAL_PIPELINES`,
the crumb ring and the frame log — are this fork's own work, and reach the tree
with the re-application of its fixes onto the adopted base.

## Known problems

- **Anything the sun does not hit directly is black** in the base's tree: the
  title's ray-traced bounce light and its lamp and sun shadows are not lit yet.
  The scaffold is in [docs/fork-ray-tracing.md](docs/fork-ray-tracing.md), is off
  by default, and by that document nothing is lit by it.
- **The first world frame crashes.** A descriptor slot holds `-1` and the title
  dereferences it, at a frame number that moves between 1,250 and 1,650 or
  does not come at all. It stands between this tree and any rendering
  measurement, and it is the one thing worth fixing first.
- **Entering the world on a real GPU can hang the card**, and Windows then
  resets the display driver (every monitor goes black for a moment, and one
  2026-11-24 hang needed a restart). Use `VICTORY_D3D12_WARP=1` for anything
  long or unattended, and a wall-clock watchdog — 120 s, 180 s at most — for
  anything on the card.
- **The D3D12 debug layer, GPU-based validation and DRED are unavailable on the
  reference PC**: Windows' Graphics Tools optional feature is not installed, so
  `VICTORY_D3D12_DEBUG`, `VICTORY_D3D12_GBV` and `VICTORY_D3D12_DRED` report
  `no D3D12 debug layer` rather than doing anything. The in-host instruments in
  [Debugging](#debugging) exist because of that.
- **Zeroed measurements taken in the menus are not measurements of the world.**
  The title stops at an Xbox-style account picker and draws nothing 3D until a
  profile is chosen. A zeroed depth target read there has already misled one
  investigation.
- Around 24 GB of memory is in use during play in the base's tree.
- On this PC the Rockstar boot movie was reported at 15 fps with broken audio in
  one 2026-11-24 build, while our own baseline measured 30 fps with
  sample-identical audio at `b77a938`; that bisect is open. Read it as a
  regression in one build, not as a property of the layer.

## Sources and attribution

### Marvisak/VIctory — GPL-3.0-or-later

The origin of everything structural: the loader that prepares, maps and binds
the title's image, the runtime services (`victory-hle`) that implement the
console's APIs, the Direct3D 12 back end (`victory-gpu`), and the shader
recompiler (`victory-shader`) that turns the console's GPU code into DXIL. Both
downstream forks keep upstream's history intact below their own commits.

### ivall/xcl — GPL-2.0-only

The project is now at [xcl-dev/xcl](https://github.com/xcl-dev/xcl);
`ivall/xcl` redirects there.

A reference for how this title behaves under another compatibility layer: it
runs console game code natively, reimplements the console's system libraries and
recompiles RDNA2 shaders to DXIL, which are the same problems this layer has.
Fixes it inspired were written anew in VIctory's code. **No code from XCL is in
this repository**, and none may be added: it is GPL-2.0-only, which does not
combine with this repository's GPL-3.0-or-later. That distinction is why this
section says "reference" rather than "source".

### fxceless/victory-gta6 — GPL-3.0-or-later

The base this fork now follows. Its commits are merged, not copied, so each one
can be read in the log. What came from it:

| Area | What it changed | Where |
|---|---|---|
| Rendering | meshlet culling read its scene buffers as the wrong kind of view, so the world drew almost nothing | `crates/victory-gpu/src/draw.rs`, `descriptor/` |
| Rendering | per-draw descriptor tables could run out, after which draws used slots never made for them and the card hung | `crates/victory-gpu/src/descriptor/host.rs` |
| Rendering | light packed by the game's compute shaders was read back through empty copies (sky, probes, fog), per mip and per layer; gathers at a named mip read level 0 | `crates/victory-gpu/src/shadow.rs` |
| Rendering | the material-program translations: fog reconstruction, shadow mask, hair, blend-shape skinning, fire, and the implicit-level samples that made compute modules invalid | `crates/victory-shader/src/emit/lower.rs`, `resource.rs` |
| Submission | submissions sent ahead of the card's wait (`VICTORY_LATE_WAIT`) with a relay thread landing results as the card finishes (`VICTORY_RELAY`) | `crates/victory-gpu/src/host.rs` |
| Submission | title memory opened as a graphics heap so buffers are used in place instead of copied (`VICTORY_INPLACE`) | `crates/victory-gpu/src/inplace.rs` |
| Ray tracing | the scaffold: builder programs translate, shader reads of title memory by raw address, and the hardware ray/box and ray/triangle test emulated in DXIL and checked against a Rust reference on the GPU | `crates/victory-shader/src/bvh.rs`, `emit/intersect.rs`, [docs/fork-ray-tracing.md](docs/fork-ray-tracing.md) |
| Stability | the hang the wave64 update introduced: its barrier form for 64-lane programs is off unless `VICTORY_WAVE_WIDE=1` | `crates/victory-shader/src/emit/wave.rs` |
| Stability | an intermittent device loss from the pipeline library file being replaced while mapped | `crates/victory-gpu/src/pipeline/library.rs` |
| Speed | clean-up passes over each translated module (`VICTORY_SHADER_OPT` bits: dead code, folding, reuse, branch facts, loads within a block, wave64 first-lane exchange), part of the shader cache key. Its author measures about 43 % fewer host instructions over the pack and about 20 ms less GPU time per frame on their machine | `crates/victory-shader/src/emit/bitcode/opt.rs` |
| Tooling | `translate` reports a census of the emitted code | `crates/victory-shader/src/emit/bitcode/census.rs`, `apps/victory/src/translate.rs` |

Its own README states the conditions on its numbers: one machine (Windows 11,
i7-14700KF, RTX 4070 12 GB, 32 GB RAM), Series X profile, and its commits were
developed with Claude Code.

### What this fork's own crew added

Each row is a landed commit on `fork/main`, with the file it lives in, the test
that pins it, and the measurement that justifies it. Where there is no number,
the row says what it protects instead.

| Change | Where | Measurement, with its conditions |
|---|---|---|
| An optimized clear value only on a texture that can take one | `crates/victory-gpu/src/resource.rs` (`host_texture_desc` → `keep_clear`) | A WARP probe: D3D12 answers `E_INVALIDARG` (`0x80070057`) for a clear value on a resource whose render-target flag the format's rules removed. Before the fix, three `no host texture` failures a session for NV12 (format 86) at 1024x4096 and B4G4R4A4_UNORM (115) at 2560x1440 |
| A view needs its resource's permission, and a clear it cannot serve is skipped | `crates/victory-gpu/src/host.rs` (`allows`, `once_render`, `clears_render_target`), `list.rs` (both clears) | The `op=clear_rtv` device removal at frame 1494, named by the breadcrumb ring (barrier, transition, resource_create, indirect, execute, dispatch, clear_rtv), is gone: a skipped clear records `b=1` instead of handing a null view to the runtime. Test: `a_depth_target_takes_no_unordered_view` |
| Mesh shaders declare no wave size | `crates/victory-shader/src/emit/lower.rs`, `emit/shader.rs` (the `PSV0` header) | In the title, WARP, one keyed soak, one commit apart: mesh pipelines refused **545 → 0** with **338** mesh modules translated in both runs. Offline, `victory translate <slice> --validate` ([docs/emit-gaps.md](docs/emit-gaps.md)): chunk `d010` emits 448 of 450 mesh programs and every emitted one validates, where none validated before; `d011` emits 427 of 427 and all validate. The two that do not emit stop at a resource-layer refusal, not a validation failure |
| Mesh output bounded by the program's declaration | `crates/victory-shader/src/emit/lower.rs` | The console's 10-bit `exp prim` indices reach 1023 while the program declares 128 vertices; a primitive naming a vertex past the declaration is culled, lanes past the declared outputs store nothing, and a group count past 65535 is clamped. Undefined behaviour on the card, so there is no failure count to quote |
| A scalar a pixel program spills through a VGPR lane | `crates/victory-shader/src/emit/lower.rs` (`v_readlane_b32` before cross-lane work) | A pixel program refused an SGPR moved through a VGPR lane and read back at a constant lane as "a cross-lane operation outside compute" with nothing crossing lanes. The resolution is constant-lane only, phi arms must agree, and the write stays under the instruction's guard |
| Bindless pointer chasing, with its host half | `crates/victory-gpu/src/draw.rs`, `crates/victory-shader/src/emit/lower.rs`, `resource.rs` | The register encodes `((dword + 1) << 8) \| slot`, so the decode is `at >> 8` after `checked_sub(256)`; an address not on 16 bytes stays unbound rather than bound into an invalid call. Lane measurement (emitted DXIL validity, not a drawn frame): dump set A 13/24 → 19/29, set B 0/15 → 6/20 |
| Remembered buffer state on the shadow | `crates/victory-gpu/src/shadow.rs` (`state`), `indirect.rs`, `list.rs` | A barrier naming a before-state the buffer was not in. An explicit transition does not decay, so the indirect buffer's UNORDERED_ACCESS and the copy's COPY_DEST are states a later barrier must leave from. Found by review, pinned by `clamp_preserves_record_offsets_and_shared_budget`; no run measurement |
| The instrument set | `host.rs` (`VICTORY_GPU_CHECK`, `VICTORY_D3D12_VALIDATE`, the crumb ring, the frame log), `draw.rs` (`VICTORY_INDIRECT_RECORD`), `pipeline/host.rs` (retry diagnostics) | What turned "the application made an invalid call" into `dispatch 1x1x1 [compute e30970f1727063fd]`, then into `submission=31371 list=0 of=1`, then into `op=resource_create a=0xb6456ee00 b=3 (invalid call, 0x887A0001)`. Each switch is in [Debugging](#debugging) with its cost |
| The test gate | `host.rs` (`the_device_tests_have_a_device`), `shadow.rs` per-copy accounting, `frame.rs` (`waits_end_on_ticks_and_count_frames`) | A green `victory-gpu` suite where every device test early-returned is a suite that asserted nothing. The other two measured the machine rather than the code: one flip per tick against a 1.67 ms clock (4 of 20 parallel runs failed on unmodified `main`), and a process-global registry every other test writes (1 of 20) |

Two more are written and measured but **not in this tree yet**, so no row above
claims them:

- **Implicit-level samples outside the pixel stage.** `image_sample`,
  `_o`, `_cl` and `_b` mapped to DXIL opcode 60/61 in every stage; outside a
  pixel shader there are no neighbouring fragments, so `dxil.dll` refuses the
  module with "features incompatible with the shader stage" and no pipeline is
  created for it. Mapping them to a level-zero `sampleLevel` outside pixel
  stages makes `015832d72477fb26` — one of the modules no run had ever bound —
  validate and create. The change is uncommitted in the shader lane's worktree
  as of this writing.
- **A session guard.** One title session per machine, held as a named object by
  the operating system rather than by a lock file, so a second launch leaves
  before it touches the GPU and a child that outlives its parent still holds the
  exclusion (`apps/victory/src/session.rs`, `VICTORY_SESSION_INHERITED`). Open
  as a pull request on its own branch, not merged.

## Building and running

Requirements: Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with shader
model 6.6 (and mesh shaders for the world), and Rust, pinned in
`rust-toolchain.toml` (1.98.1, `x86_64-pc-windows-msvc`).

The title also needs several GiB of available Windows commit at startup — check
**Committed** under **Task Manager → Performance → Memory**. If `crashes.log`
reports Windows error 1455, close other applications or enlarge the paging file
and retry.

```powershell
cargo xtask install-hooks                # asset-policy check on every commit
copy victory.example.toml victory.toml   # then set your paths (outside the repo)
cargo build --release -p victory
cargo run -p victory -- config           # check the paths you just set
```

Point `victory.toml` at your own copy of the game, outside the repository — the
tools refuse a path inside it. `VICTORY_GAME_DIR`, `VICTORY_CACHE_DIR`,
`VICTORY_DATA_DIR` and `VICTORY_TEST_TITLES` override the file, and
`VICTORY_CONFIG` selects a different file. Derived data goes to
`%LOCALAPPDATA%\victory\cache` and the title's saves, settings and
`crashes.log` to `%LOCALAPPDATA%\victory\data` (the log is under
`<data>\<build id>\temp\`).

Run the title, and give it a bound:

```powershell
target\release\victory.exe run "<game dir>\<the title's exe>" --execute
```

`run` without `--execute` binds the imports and stops, which is the cheap check
that your files and paths are right. `--unimplemented trap|zero` decides what an
unimplemented import does when called, `--strict` disables host pass-through,
`--console series-x|series-s` picks the profile the title sees, and everything
after `--` goes to the title's command line. The window caption shows the
framerate.

Tests need no game data and run in CI (`.github/workflows/ci.yml`: clippy with
`-D warnings` on both feature sets, then the suite):

```powershell
cargo test --workspace
cargo test --workspace -- --test-threads=1   # the result that gates a merge
```

Two assertions in `victory-gpu` have measured the clock and a process-global
registry rather than the code (`frame::tests::waits_end_on_ticks_and_count_frames`
and the shadow snapshot tests). Both were rewritten, and a serial run is what a
merge decision reads: a compile error prints neither `FAILED` nor a
`test result:` line, so green means a `test result: ok` you read for every crate,
never the absence of a failure.

Input is XInput pads. With none connected, the keyboard is a pad while the
title's window has focus:

| Keys | Pad |
|------|-----|
| W A S D / I J K L | left / right stick |
| arrows | D-pad |
| Enter, Backspace, X, Y | A, B, X, Y |
| Q / E, Z / C | shoulders, triggers |
| F / H | stick clicks |
| Esc, Tab | Menu, View |

Offline subcommands, none of which start the title or need a GPU session:

```
victory inspect <exe|dll> [--imports] [--exports]
victory census [paths...] [--missing] [--strict]
victory lib-exports <import.lib> [--list]
victory shader-info [paths...] [--verbose] [--disasm]
victory translate [paths...] [--validate] [--pipelines] [--list] [--jobs N]
victory config
```

## Debugging

A release build has no debugging aids. Build them in with

```powershell
cargo build --profile diag -p victory --features diagnostics   # target\diag\victory.exe
```

The switches are grouped by what you would reach for. Everything marked
*diagnostics* exists only in that build, except `VICTORY_D3D12_VALIDATE`, whose
code is compiled in either way.

### What is on screen

| Variable | Effect |
|---|---|
| `VICTORY_CAPTURE=<dir>` | *diagnostics.* Saves each presented frame whose number is a power of two as `frame-<n>.png`, at half size |
| `VICTORY_CAPTURE_FRAMES=<n>,<n>` | *diagnostics.* Additional frames to save, and their draws traced at debug level |
| `VICTORY_DUMP_TARGETS=1` | *diagnostics.* With a capture folder: every texture the listed `VICTORY_CAPTURE_FRAMES` frames render to or write, saved at that frame's present as `frame-<n>-<i>-<resource>-<w>x<h>-f<format>.png`. Floats are scaled by their largest value and depth by its range, so structure shows. Only the listed frames — a power-of-two frame alone dumps nothing |

### What the GPU did

| Variable | Effect |
|---|---|
| `VICTORY_GPU_CHECK=1` | *diagnostics.* Asks the device after every recorded host operation whether it has been removed, so the first operation after which it has names itself: `the device was removed right after op=clear_rtv a=… b=…`. One device query per operation, so it is very slow — never compare frame times with it on |
| `VICTORY_D3D12_VALIDATE=1` | One submission per command list, each asked for the removal reason as it lands; the first that comes back removed is logged as the submission that removed the device with that list's newest operations. It removes the overlap between lists, so a removal needing two lists in flight will not reproduce under it: take the list it names as guilty, not the whole story |
| `VICTORY_INDIRECT_RECORD=1` | *diagnostics.* Logs an indirect execution's record: kind, stride, count, the words the title left, the thread count or draw count they name, and whether the range fits the buffer. The first 64 executions and then every 256th, so the log stays readable |
| `VICTORY_GPU_TIMING=1` | *diagnostics.* Runs and times each list of a submission on its own, and logs slow ones with their slowest commands |
| `VICTORY_SKIP_PIPELINES=<hash>,…` | *diagnostics.* Omits draws and dispatches, indirect ones included, whose pipeline contains a listed 16-digit shader hash — the way to subtract a suspect pipeline and confirm cause rather than neighbour |
| `VICTORY_D3D12_DRED=1` | *diagnostics.* Enables device-removed data: the allocation a fault hit and the commands in flight |
| `VICTORY_D3D12_DEBUG=1` | *diagnostics.* The host debug layer (needs Windows' Graphics Tools) |
| `VICTORY_D3D12_GBV=1` | *diagnostics.* With the debug layer: GPU-based validation without state tracking, so out-of-range descriptor indices become messages naming the command instead of page faults. Slow |
| `VICTORY_INDIRECT_RECORD=1` | logs an indirect execution's record: kind, stride, count, the words, the thread count they name, and whether the range fits the buffer. The first 64 and then every 256th. Reads the host copy as uploaded; the clamp's own counters, not this log, are the trustworthy group count |
| `VICTORY_NO_ARGS_REFRESH=1` | **a probe, never an option:** skips the upload of the host copy over a title argument buffer before an indirect execution. That copy is the only path a title-written record has to the GPU, so with it off every mesh dispatch asks for zero groups (measured: 102 of 102 summaries at zero, against 11,879 groups a frame with it on). It answered whether we overwrite records the title wrote: we do not |
| `VICTORY_D3D12_WARP=1` | *diagnostics.* The host device on the software adapter: a shader's wild access becomes an access violation in this process instead of a GPU fault and driver reset. Slow, and it cannot black the machine, which is why soaks and unattended runs use it |

### What the shaders did

| Variable | Effect |
|---|---|
| `VICTORY_SHADER_LOOP_LIMIT=<n>` | *diagnostics.* Translated loops stop after `n` iterations per lane (decimal or `0x` hex, at least 1) instead of 2^20. The value is part of the translation, so it takes its own cache entries. If a lower `n` makes world frames much faster, loops hitting the limit are what makes them slow |
| `VICTORY_SKIP_PIPELINES=<hash>,…` | as above: omits work whose pipeline contains that shader |
| `VICTORY_SERIAL_PIPELINES=1` | Creates compute pipelines one at a time, to tell a driver race from a description we build differently each run. A refusal that a later attempt at the same module survives is not a verdict on the module |

### What the process did

| Variable | Effect |
|---|---|
| `VICTORY_LOG=<target>=<level>,…` | Log filter, e.g. `VICTORY_LOG=victory_loader=trace,info`; falls back to `RUST_LOG`. A release build cannot log below info whatever the filter asks |
| `VICTORY_STACKS=<s>,<s>` | Every thread's unwound stack as `module+rva`, that many seconds after start, to see what a title waits on |
| `VICTORY_WATCH=<rva>,…` | Polls the u32 at each title address (hex) every 5 ms and logs it whenever it changes — an engine's state, say |
| `VICTORY_CRASH_SAVE=<rva>:<len>,…` | Writes those title ranges (hex) to `crash-<rva>.bin` in the cache folder on a crash |
| `VICTORY_KEYS=<second>:<key>,…` | *diagnostics.* Presses each key for 0.2 s at that second after input starts, focus or not. A key is a letter, a digit, or `Enter`, `Esc`, `Tab`, `Back`, `Up`, `Down`, `Left`, `Right`. This is how the account picker gets answered without a person: cover the whole window, `VICTORY_KEYS=5:Enter,10:Enter,15:Enter,…` out to five minutes, because the picker can ask twice |
| `VICTORY_AUDIO_CAPTURE=<file.wav>` | Records what the title plays, from our process only, as 48 kHz 16-bit stereo. It starts a child of the title to do the recording |
| `VICTORY_XMA_DUMP=<dir>` | Saves the first 32 XMA voices' context and input |

Every build appends crashes and host GPU failures to `crashes.log` in the
title's data folder.

### What a frame costs

These change what runs, not just what is logged. All are off by default unless
the row says otherwise, and the base's README is where their measurements live
(RTX 4070, in the world).

| Variable | Effect |
|---|---|
| `VICTORY_PIPELINE_WAIT_MS=<n>` | The per-frame budget, over all threads, for waiting on background pipeline creation. Default 500 ms; a draw can be omitted until its pipeline is ready |
| `VICTORY_LATE_WAIT=1\|2\|3` | Sends a queue's submissions without waiting for each, and settles them at the next point a title buffer, a fence or a present is observed. Off unless the value is exactly 1, 2 or 3, and forced off by `VICTORY_GPU_TIMING`, which has to wait for each list to time it |
| `VICTORY_RELAY=<1\|2>` | A thread of its own lands finished flights as the card runs them, so the submitting thread never waits and copies back itself. `2` also waits only for the flights that declared a range, not for everything in the air |
| `VICTORY_WIDE_BACK=1` | Takes a submission's shader-written snapshots together and compares everything declared after the card, cut into ~1 MiB pieces over a few threads, instead of range by range on one |
| `VICTORY_CARD_BUFFERS=0` | Turns off card-side copies of large title buffers, which are on by default. `VICTORY_CARD_BUFFERS_MIN_MB` (16) is the smallest buffer that gets one and `VICTORY_CARD_BUFFERS_MB` (512) the most bytes spent on them |
| `VICTORY_INPLACE=<1\|2>` | Opens the title's own memory as a graphics heap and places buffers over it, instead of copying. `1` covers private windows outside buffers, `2` title buffers too — and `2` must not be combined with late wait, since a flight's rules know shader writes only by declared ranges |
| `VICTORY_BVH_BUILDER=1` | Lets the ray-tracing builder's stages run, which they do only while every builder pipeline the title created has a host pipeline. `VICTORY_BVH_QUARANTINE=1` also keeps the builder's stores out of title memory, for a first test. Nothing is lit by ray tracing yet |

One session at a time is a rule of the work, not a switch: the title shares one
GPU, one save folder and one shader cache, so two sessions at once is the one
failure this project cannot recover from — it blacked the reference machine
once. `translate`, `inspect`, `census`, `shader-info` and `lib-exports` are
offline and take nothing.

## Licence

**GPL-3.0-or-later.** See [LICENSE](LICENSE).

Everyone in the chain — upstream VIctory, fxceless/victory-gta6, and this fork —
licenses under the same terms, and the merged history keeps each author's work
identifiable by commit.

Redistributing this, in source or as a binary, means: ship the complete
corresponding source of the whole combined work under GPL-3.0-or-later, keep the
licence text and the copyright notices, and add no further restriction on top of
it. There is no warranty, and no game content is licensed here — the game stays
the user's own copy, obtained by them, and none of it is distributed with this
repository.
