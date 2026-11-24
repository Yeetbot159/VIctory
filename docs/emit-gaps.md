# The instruction tail: what the emitter still refuses, and how many programs it costs

The counts below come from the shader archive, not from a run's dump set.
`victory shader-info D:\Grand Test Auto6.Xbox.v1.0.2.1\shaders.rpf` finds
**26,503 containers and 26,698 native programs** (Vs 2,236, Ps 12,101,
Ms 9,282, Cs 2,689, Es/Gs 195 each) in about 9 seconds: the file is SGD5, and
each record is one deflate-packed container. A `VIctory-data\failed-shaders`
dump describes one session, and one session touches a fraction of a percent
of them.

19,506 programs over 20,000 containers, translated one slice of 500
containers at a time (`victory translate <slice> --list`; a single run over
the whole archive outlives the tool's per-call budget):

| Stage | emitted | of | missing |
| --- | ---: | ---: | ---: |
| vertex + pixel | 7,504 | 8,416 | 912 |
| mesh | 8,383 | 8,713 | 330 |
| compute | 2,245 | 2,377 | 132 |

Refusals by family and by owner:

| Programs | Refusal | Owner |
| ---: | --- | --- |
| 808 | emit: a descriptor's address or format read as data | resources |
| 207 | emit: a lane mask read as data | this file, below |
| 195 | resources: an address in an SGPR that is no root parameter | resources |
| 174 | emit: a packed input half no element holds | I/O |
| 167 | I/O: an interpolation with computed barycentrics | I/O |
| 64 | resources: an address not from a root parameter | resources |
| 63 | emit: a descriptor read only as data | resources |
| 41 | emit: a descriptor chosen by control flow | resources |
| 38 | resources: a descriptor read from the first lane of computed VGPRs | resources |
| 15 | resources: a waterfall loop whose compares do not select the access's lanes | resources |
| 15 | emit: a hardware SGPR input | instruction |
| 13 | emit: a constant buffer's descriptor read as data | resources |
| 13 | emit: a constant lane mask | instruction |
| 12 | emit: a lane mask holding lanes elsewhere read as data | instruction |
| 8 | resources: a descriptor partly joined by control flow | resources |
| 8 | emit: the first half of an interpolation | I/O |
| 5 | emit: a whole-wave section step the simulation does not follow | instruction |
| 4 | emit: `s_getpc_b64` | instruction |
| 4 | emit: an LDS operation | instruction |
| 4 | emit: `v_cvt_f32_f64` | instruction |
| 4 | resources: an address from a phi | resources |
| 3 | resources: a descriptor neither loaded nor built | resources |
| 1 | emit: `v_permlanex16_b32` | instruction |
| 1 | emit: LDS the program does not allocate | instruction |
| 1 | emit: a lane-mask join of an untracked mask | instruction |

## Closed on this branch

**A scalar spilled through a VGPR lane, read back at a constant lane.** The
console compiler moves an SGPR through a VGPR lane and reads it back:

```
v_writelane_b32 v158, s14, 1
v_readlane_b32 s0, v158, 1
s_load_dwordx8 s[72:79], s[90:91], s0
```

`pixel-ee41ebc3eb3fbbdc` feeds a descriptor offset this way, and a pixel
program refused the lane write with `a cross-lane operation outside compute`
even though nothing crosses lanes: at a *constant* lane the read back is the
scalar the write stored. `lower::valu_op` resolves it through `spilled_value`
and `spilled_in_vgpr`, the pair the lane-mask spill already uses.
`victory translate pixel-ee41ebc3eb3fbbdc.dxbc --validate` reports
`1 emitted of 1, 1 validated`, and `a_pixel_shader_reads_a_scalar_it_spilled_through_a_lane`
fails on the tree without the change. With this, every pixel container in both
dump sets translates; the only refusals left there are five mesh programs in
the resource layer.

**A mesh shader no longer asks for a wave size.** DXIL allows the wave-size
attribute only on compute and node shaders:

```
error: Function: main: error: WaveSize only allowed on compute or node shaders
```

Every mesh program in this archive is wave64 (920 sampled, none wave32), and
the emitter set `wave_size = Some(64)` for each of them, so **every emitted
mesh shader failed `--validate`** — 8,383 programs — and the container's
`PSV0` lane counts disagreed with the module even after the attribute was
dropped (`DXIL container mismatch for 'PSVRuntimeInfo'`). Neither is now
emitted for a mesh stage.

Measured with `victory translate <slice> --validate`: chunk `d010` emits 448 of
450 mesh programs and **every emitted one validates** (0 did before); `d011`
emits 427 of 427 and all validate; the vertex and pixel programs alongside are
50 of 50 and 73 of 73. The two programs in `d010` that never emit are refused
before emission by the resource layer — `05276.dxbc@0x0 Ms at 0x5ee4` and
`05278.dxbc@0x0 Ms at 0x60e8`, both `a descriptor read only as data`, the
family this table counts 63 of — so neither is a validation failure.
`translate --validate --pipelines` on a mesh program reports **1 validated,
1 pipelines**: host mesh pipelines still create without the claim.
`a_mesh_shader_declares_no_wave_size` holds `PSV0` to "any".

The consequence is stated where it belongs: a mesh program cannot ask the
runtime for the console's wave, so its lanes are the device's.

Removing a declared wave size would be alarming if any lowering depended on the
host wave, so here is why none does — checked, not assumed.

- `lane_index` (`emit/lower.rs:2407`) computes a mesh lane as
  `flat & (wave - 1)` from the group thread index, so it is the **console's**
  lane whatever the host wave is; a mesh program never asks the host for it.
- `cross_lane` (`:2276`) refuses every cross-lane operation in a mesh program,
  so no ballot, lane read or DPP is emitted for one.
- `v_mbcnt` (`:6601`) takes its non-Host branch for mesh without needing
  cross-lane: it derives the count from that console lane index and the
  console's mask.
- Compute keeps its declaration — `:562` is the only remaining
  `wave_size = Some(wave)` — and the entry-point property is filtered on
  `mesh.is_none()` (`emit/shader.rs`).

## Open, with what would settle each

**A lane mask read as data (207 mesh programs).** The console saves a mask to
a scalar (a register pair in wave64) and reads it back as a number:
`v_cmp_eq_u32 vcc, s14, v80` / `s_xor_b64 s[12:13], vcc, exec` /
`s_bcnt1_i32_b64 s12, s[12:13]` / `s_cbranch_scc1` is a wave-wide "does any
lane differ from the first lane's value", and the count decides a uniform
branch. Compute programs already do this exactly (`MaskData::Console`,
because they declare the console's wave size). For mesh, DXIL cannot declare
one — see above — so with a host wave narrower than the console's the two
halves of a console wave would answer separately and the "uniform" branch
would stop being uniform. The honest fix is group-shared memory: each thread
writes its bit to LDS by its flattened thread id, one barrier, then each
thread counts the bits of its console wave. That is exact and costs a barrier
per site. What would settle whether it is needed at all: the device's mesh
wave size, read back from a compute shader that writes `WaveGetLaneCount()` to
a UAV in the same run — if the driver runs 64-wide waves for the mesh stages
in these pipelines, the ballots are already the console's and no emulation is
needed.

**A packed input half no element holds (174 pixel programs).** A 16-bit pixel
input is packed two per dword and the host's inputs are 32-bit floats, so
`lower` rebuilds each half from the input elements (`packed_half`), which
returns `None` when no element names that half.

Diagnosed on `chunks/c005/11190.dxbc@0x0`. The console's input record for
attribute 1 says `row 3, pack2, row2 == row`, and the program interpolates that
attribute at channels x, y and z, each in both halves (`v_interp_p1ll_f16` and
`v_interp_p2_f16 … high`) — six halves. Our element for row 3 declares
`cols 4` halves, so half 4 has no element and the refusal fires. The console
reads halves the row carries and we do not name: this is a **declaration**
bug, not a source we lack.

The console's own declaration contradicts both readings, which is why this is
diagnosed and not fixed. For `c005/11190.dxbc` the pixel `ISG1` holds

```
TEXCOORD[1]  row=3  mask=0xF  type=6 (16-bit)
TEXCOORD[4]  row=4  mask=0x3  type=3
TEXCOORD[5]  row=4  mask=0xC  type=3
TEXCOORD[6]  row=5  mask=0x7  type=3
```

and the program interpolates halves 0-5 of the row-3 attribute. `mask` is a
four-bit field, so read as halves the element holds four and is two short of
what the program reads; read as 32-bit registers it holds four *dwords*,
registers 3-6, which collides with `TEXCOORD[4]`/`[5]` at row 4 and
`TEXCOORD[6]`/`[7]` at row 5. Neither reading covers the read, so the
attribute index an interpolation names is not simply the signature's
`Register` field, or the 16-bit mask counts in a space the signature does not
describe. A widening built on either guess changes what the host interpolates
for a live attribute, which is the black-scene class rather than a refusal.

So the honest state of this family is the loud refusal it already produces,
and the next step is to identify that index space from a pair that works: a
program whose 16-bit attribute reads halves past its `mask`, next to the
vertex stage that writes it.

The rule any version of it must keep: a pixel input never silently reads zero,
and a width that decodes a neighbour's dword is the black-scene class rather
than a clean refusal.

**An interpolation with computed barycentrics (167 pixel programs).**
Diagnosed, and it is not a phi the io pass merely failed to follow. On
`chunks/c005/10550.dxbc@0x0` the barycentric phi chain resolves
(`11035 → 10942 → 10846`, every arm `v1` as `PerspectiveCenter` index 1) down
to a leaf that is `v_mul_legacy_f32 at 0x3580` reading `V(1)`: the console
**computes** its own interpolation weight. The disassembly agrees — `v0`/`v1`
are the PerspectiveCenter pair at 0x10a0 and 0x1eac, arithmetic overwrites
them at 0x1ebc/0x1edc (`v_mul_legacy_f32 v0, s18, v8`, `v_mul_legacy_f32 v1,
s36, v13`, then `v_fmac_f32`), and the same `attr2` is interpolated off those
registers again at 0x377c/0x3790. An agreement walk over those arms returns
`Err`, correctly: they do not name the same value.

The clobber is on **one arm only**, which rules out "we have the wrong idea
about what that register holds": four branches jump past it straight into the
second interpolation —

```
000ea4: s_cbranch_execz label_35BC
000ee0: s_cbranch_execz label_35B8
0016c4: s_cbranch_execz label_35A0
001a74: s_cbranch_execz label_35A0
```

and 0x35a0-0x35bc falls into 0x377c/0x3790. So one path reaches
`v_interp_p2_f32 v28, v1, attr2.z` with `v1` still the PerspectiveCenter and
another reaches it after `v1` was overwritten. The same attribute is
interpolated twice, once off the barycentric pair and once off a computed
value, and only the first is an interpolation at all.

So one attribute is interpolated two ways and one of them uses weights the
shader built itself, which `SV_Barycentrics` as an interp mode cannot express.

**How many of the 167 are the cheap shape: none.** Every one of the 167 writes
`v0` or `v1` with an ordinary instruction (`v_mul_legacy_f32`, `v_fma_f32`,
`v_mov_b32`, `v_cndmask_b32`, `v_cvt_pkrtz_f16_f32` and friends — scanned over
the whole set from the disassembly), so no agreement walk resolves any of them
and the family is entirely the pull-model case: read the attribute's values and
interpolate by hand, which the console data already distinguishes
(`Barycentric::PerspectivePullModel`, for which `interp()` returns `None`) and
which the emitter does not implement. Nothing here is a phi that was missed.

**The rest** are single programs each (`s_getpc_b64`, `v_cvt_f32_f64`, an LDS
operation, `v_permlanex16_b32`, a whole-wave section step); each is a small,
self-contained instruction to add once something depends on it.

## One compute module the validator refuses

`015832d72477fb26` (a 99 KB Cs, wave32) emits, creates a PSO,
and fails DXIL.dll: `Function: main: error: Function uses features
incompatible with the shader stage (cs) of the entry function`. It is one of
the 35 compute hashes a keyed run never brought into existence, where the
other 585 refused hashes in the same log bind a pipeline minutes later.

Three things it is not, each measured rather than argued:

- **Not its feature declaration.** Clearing `SFI0`'s WAVE_OPS (`0x4000`),
  INT64 (`0x8000`), and both heap bits (`0x600_0000`) in four variants — no
  clear, each alone, both together — gives the identical error.
- **Not a stage-illegal `dx.op`.** Against a compute module that validates, its
  op set is unique in fourteen entries (`bufferLoad`/`bufferStore`,
  `flattenedThreadIdInGroup`, `legacyF32ToF16`/`legacyF16ToF32`, `sample`,
  `sampleCmpLevelZero`, `unary.f32 28`, `unaryBits`, and five wave ops), every
  one legal in a compute shader and every opcode correct for its name.
- **Not its entry properties.** `threads=[32,1,1]`, `wave=32`, no input or
  output signature, no barycentrics.

So it is in the call graph or the metadata the entry point does not carry. A
class bisect of the emitter's op calls (a temporary `VICTORY_OP_SKIP` hook that
substitutes a typed zero) excludes three of the ten candidate classes — the
unary ops, the two legacy f16 conversions, and `flattenedThreadIdInGroup` all
leave the original error unchanged when zeroed — and leaves four inconclusive:
`sample`/`sampleCmpLevelZero`, `bufferLoad`/`bufferStore`, `textureGather`/
`textureLoad`, and the wave ops. Those four return vectors, and a vector return
has no typed zero here, so the hook falls back to `undef` and the validator
reports `Instructions should not read uninitialized value` before it reaches the
stage check. The refinement is a zero initializer for an arbitrary type; without
one those four cannot be separated by this method.

The four variants were run through a temporary `VICTORY_SFI0_CLEAR` mask in the
emitter; it is not in the tree.

## The lane-mask join, after inspecting it

Five compute programs in `c012` refuse `a lane-mask join of an untracked mask`.
Instrumenting the refusal shows every one of them joining a value that no
instruction writes:

```
join: arg=Value(2453) def=None block=5  off=0x568
join: arg=Value(6625) def=None block=39 off=0xbc0
join: arg=Value(6586) def=None block=5  off=0x520
join: arg=Value(4307) def=None block=38 off=0xc40
join: arg=Value(7004) def=None block=5  off=0x508
```

The refusal reads `st.masks` directly (`lower.rs:8670`) rather than
`st.mask_bits(low, high)`, which is how every other mask use resolves a Wave64
pair, so the obvious correction was to use the accessor. It was tried and it
does not resolve these: `c012` still emits 228 of 234 compute programs. So these
values are in neither `masks` nor `paired_masks` — they are entry or phi values
the mask analysis never binds — and a join of a mask nothing tracks is exactly
where the refusal is the right answer rather than a width to widen.
