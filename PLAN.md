# Racer: Project Plan

A multiplayer street racing game set in a fictional city. The target is GTA V/VI visual quality or better, with realistic driving physics. It runs on PC (Windows, Linux, macOS), with races of up to 8 players or solo against AI. All cars and tracks are unlocked from the start and there is no progression. The build is done by AI agents, and assets come from generative pipelines.

## 1. Constraints that shape everything

| Decision input | Answer | Consequence |
|---|---|---|
| Who builds it | AI agents, large token budget | Workflows must be text-first and scriptable. The agent has to be able to see its results (screenshots, profiler data) without a human in the loop. |
| Platforms | Windows, Linux, macOS (Apple Silicon) | A Metal-capable, high-end renderer that runs on all three. No console or mobile compromises. |
| Lobby size | 8 | Server-authoritative physics with prediction is affordable, and every car can be fully simulated on the server. |
| City | Fictional | No licensing or real-world data constraints. The city can be designed around the tracks. |
| Cars | Fictional | No manufacturer licensing. Designs can be "inspired by" without trade dress. |

## 2. Technology choice

**Engine: Unreal Engine 5 (latest stable 5.x at kickoff), C++ only, no Blueprint logic.**

Nothing else on the market reliably reaches GTA VI-class visuals on PC in 2026. Nanite handles film-density city geometry, Lumen with hardware ray tracing handles global illumination and reflections (wet asphalt at night is the signature look), and MegaLights covers thousands of street and neon lights. Substrate gives layered car paint (clearcoat, metallic flake), and Virtual Shadow Maps and TSR/DLSS/FSR/XeSS cover the rest. UE5 ships on all three PC targets, including Metal on Apple Silicon. Bevy and Godot were considered and rejected: neither has a production-grade answer for Nanite-scale geometry plus real-time GI at this fidelity, and closing that gap would turn the project into an engine project.

The usual problem with UE for AI development is its GUI- and binary-asset-centric workflow. The plan works around it (section 8): gameplay in C++, content built by Python editor scripts and commandlets, data in text (JSON/DataTables imported from CSV), and editor-free headless builds and tests.

**Where Rust is involved (deliberately, not decoratively):**

| Component | Language | Why |
|---|---|---|
| Vehicle dynamics core (`racer-dynamics`) | Rust, linked into UE via the `cxx` crate as a static library | Tire model, suspension, drivetrain and aero form a pure, numerics-heavy function of state. As a standalone crate it is unit-testable and benchmarkable headlessly in milliseconds, and it can be fuzzed for NaNs and blow-ups. It is shared bit-for-bit by client and dedicated server. This is the part of the game that most benefits from an AI iterating fast against tests. |
| Backend services (matchmaking, lobbies, leaderboards, server fleet orchestration) | Rust (axum, tokio, sqlx, Postgres) | Small, correct, cheap to run. |
| Content tooling (track/road network compiler, asset validation, CI tools) | Rust CLI + Python for Blender/UE scripting | Fast validators for the data that drives level generation. |
| Rendering, gameplay, netcode, UI | C++ in UE | Engine-native. Fighting the engine here costs more than it gains. |

If the Rust FFI boundary ever becomes a liability, `racer-dynamics` is small enough (a few thousand lines) to port to C++ without design changes. That is the escape hatch.

**Platform services:** Steam (Steamworks) for distribution, identity and overlay, since it covers all three OSes. Easy Anti-Cheat via Epic Online Services (supports Windows, Linux/Proton and macOS). Dedicated servers run as Linux containers on a game-server host (Edgegap, Hathora or AWS GameLift, picked on price at the beta).

## 3. Game design

**Modes:** Circuit (laps), Sprint (point to point), Time Trial (solo, ghosts), and Free Roam of the track corridor for practice. Every mode is available solo vs AI and online. Online lobbies can be public matchmade or private with an invite code, and humans can be mixed with AI fill.

**Race options (all available from the start):** time of day (dawn, day, golden hour, night), weather (dry, overcast, rain, wet-drying), track direction (forward and reverse), traffic (off, light; ambient AI cars on non-racing lanes as obstacles), collisions (full, ghosted for the first lap, off), damage (cosmetic or cosmetic + mechanical), AI difficulty, and assists (ABS, TCS, stability control, steering assist, racing line). Assists are per player and shown in the results screen so times stay comparable.

**Contact:** Ramming is first-class. Car-to-car contact is fully physical. Damage is visual (mesh deformation, detachable bumpers, mirrors, hoods and doors, broken glass and lights). With mechanical damage on, contact also bends steering alignment, cuts engine power and punctures tires. A "wreck" (car flipped or stuck for 3 s) triggers a reset to the last valid track position after a short delay.

**Content at launch:**

| Content | Count | Notes |
|---|---|---|
| Cars | 16 fictional cars across 6 classes | Hot hatch, sport sedan, muscle, sports/GT, supercar, hypercar. Each class has 2 to 3 cars with distinct handling (FWD/RWD/AWD, mid/front engine). Races can be class-locked or open. |
| City districts | 4 | Downtown (glass towers, tight 90° grids, tunnels), Harbor (docks, containers, wide industrial roads, a drawbridge), Hills (switchbacks, canyon road, overlook), Old Town/Strip (neon, narrow streets, cobbles). |
| Tracks | 12 base layouts (3 per district) | Each also runs in reverse, for 24 drivable layouts, and each supports all time and weather combinations. |
| Liveries | 8 per car + color picker | Purely cosmetic. Also unlocked from the start. |

**AI difficulty (5 levels):** Novice, Amateur, Pro, Expert, Alien. The levels scale racing-line precision, braking-point margin, corner-speed fraction, mistake rate, overtaking aggression and contact willingness (an Alien rams back, a Novice avoids contact). There is no rubber-banding by default, with a lobby option for mild catch-up. AI drives the exact same physics as players, with no grip or power cheats.

## 4. Vehicle physics (realism core)

The Chaos rigid body owns the chassis (mass, inertia tensor, collisions with the world and other cars). `racer-dynamics` computes all wheel, tire, suspension, drivetrain and aero forces, which are applied to the chassis in UE's async physics tick.

| Subsystem | Model |
|---|---|
| Physics rate | Fixed 240 Hz chassis step. Wheel spin and drivetrain substepped ×4 (960 Hz) inside Rust for stiff-system stability. |
| Tires | Pacejka Magic Formula 5.2-style combined slip (longitudinal + lateral via friction ellipse/combined slip weighting), load sensitivity, camber thrust, relaxation length for transient response. Surface grip multipliers per physical material (asphalt, wet asphalt, cobbles, painted lines, metal plates, grass, dirt). Temperature/wear are out of scope. |
| Contact | Wheel shape-casts (not single raycasts), so curbs and potholes read correctly at speed. |
| Suspension | Per-wheel spring, bump/rebound damper, bump stop, anti-roll bars. Geometry approximated via camber/toe curves vs travel. |
| Drivetrain | Torque curve per engine, rev limiter, flywheel inertia, clutch, gearbox (manual/auto/sequential), differentials (open, LSD with preload/ramps, locked, AWD center split). |
| Aero | Drag, downforce front/rear with ride-height sensitivity, slipstream from cars ahead (sampled from the server's car positions). |
| Assists | ABS, TCS, ESC implemented as controllers inside `racer-dynamics`, so they behave identically for AI and players. |
| Input | Keyboard, gamepad (with steering filtering/speed-sensitive steering when steering assist is on), and steering wheels with force feedback via SDL3 (Windows, Linux, macOS) driven by the self-aligning torque from the tire model. |

Each car is a `car.toml` in the repo (mass, CG, inertia, suspension, tire coefficients, torque curve, gear ratios, aero), so an agent can tune it from text. Validation is automated with a set of reference maneuvers run headlessly in `cargo test`: 0 to 100 km/h, 100 to 0 braking distance, steady-state skidpad g, step-steer response, and a lap time on a reference track spline. Each car has target bands per class, so a tuning change that breaks realism fails CI.

## 5. Multiplayer architecture

**Topology:** dedicated, server-authoritative Linux servers, one process per race (8 players + AI fill). The server runs the same physics at 240 Hz and sends snapshots at 60 Hz.

**Netcode:** UE's networked physics with resimulation (Chaos physics prediction). The local car is fully predicted. On correction, the client rewinds to the server's state and resimulates its buffered inputs. Remote cars are extrapolated from the latest snapshot plus their last input (throttle/steer/brake are sent, which predicts far better than pure dead reckoning) and smoothed over 100 to 150 ms. Car-to-car contact is predicted locally against the extrapolated remote car and authoritatively resolved by the server. Corrections are blended visually over a few frames so ramming feels physical without snapping. Target: playable to 150 ms RTT. Above 250 ms the player gets a warning and matchmaking prefers lower-latency regions.

**Fairness:** the server validates inputs, not positions, so speed hacks and teleports are structurally impossible. Collision griefing is mitigated by the ghosted-first-lap option and a vote-kick in public lobbies. Easy Anti-Cheat covers memory tampering and input bots.

**Backend (Rust):** Steam auth ticket → session token. Matchmaking by region, mode and class, with skill-agnostic quick play plus optional private lobbies. Allocation of a server container per race. Per-track/per-class/per-assist-level lap-time leaderboards with replay ghosts stored in object storage. Leaderboards are not progression, they only rank times.

**Replays:** both server and client record input streams + periodic state keyframes. These provide full race replays with free camera and ghost data for Time Trial.

## 6. Graphics plan

| Area | Approach |
|---|---|
| Lighting | Lumen with hardware ray tracing on capable GPUs (software Lumen fallback), MegaLights for street/neon/headlights, Virtual Shadow Maps. Time of day via a sky atmosphere + volumetric clouds setup, with baked "look" profiles per time/weather combination tuned by color-graded reference images. |
| Wet roads | Weather drives a global wetness parameter: material-level puddle masks, darkened albedo, roughness drop, ray-traced/Lumen reflections, rain particles, tire spray, wipers. This is the single biggest "GTA VI" visual signal and gets dedicated polish time. |
| Cars | Substrate car paint (base + metallic flake + clearcoat), real glass with refraction, interior visible through windows, emissive light clusters with lens-flare-free bloom, tire sidewall deformation, brake disc glow, dynamic dirt and water accumulation. LOD0 around 300 to 500k triangles, Nanite where supported for static parts (Nanite does not deform, so skinned/deformable panels stay standard meshes with LODs). |
| Damage | Pre-authored deformation via vertex-offset damage zones driven by impact impulses, plus detachable parts as physics bodies. Chaos Destruction is used only for breakable props (fences, bins, signs, glass). |
| City | Nanite everything static, World Partition streaming, high density at track corridors, impostor/HLOD skyline beyond. Ambient pedestrians on sidewalks behind barriers (non-interactive, flee animation), ambient traffic optional. |
| Camera/Post | TSR/DLSS/FSR/XeSS, physically based motion blur, depth of field in replays only, camera shake from suspension. Chase, hood, bumper and cockpit cameras. |
| Audio | Granular engine synthesis from recorded/generated RPM loops per engine type (MetaSounds), tire squeal driven by slip ratio and angle, impact sounds driven by impulse, convolution reverb per district (tunnels!). |

**Performance targets:** 1080p/60 at High on an RTX 3060/RX 6700 class GPU. 1440p/60 at Ultra with hardware RT on an RTX 4070 class GPU. 4K/60 with upscaling on an RTX 4080+. Apple M2 Pro or later at 1440p/60 with MetalFX/TSR at High, software Lumen. Minimum 16 GB RAM and an SSD (required for streaming).

## 7. Content pipeline (AI-generated assets)

All asset generation runs as scripted pipelines whose inputs live in the repo as text, so an agent can regenerate or iterate any asset.

**Cars:** design brief (`cars/<id>/brief.md`) → concept images from an image model (multiple angles, consistent design) → image-to-3D for the base form → **Blender headless Python pipeline** (retopology to clean quads, separation into body panels, doors, hood, bumpers, wheels and glass, UV unwrap, LODs, collision hulls, socket placement for wheels/lights/exhaust) → material assignment to the shared Substrate car-paint master material → UE import via Python. The hand-quality bar is set by a checklist validator (triangle budgets, part naming, wheel radius matching `car.toml`, no non-manifold geometry, UV density range).

**City:** the city is described as data. `city/roads.json` is a road graph (nodes, lanes, widths, elevation, bridges, tunnels). `city/districts/*.toml` holds building style rules, density and prop palettes. `tracks/<id>/track.toml` holds the route over the road graph, checkpoints, start grid, barriers, shortcuts and spectator areas. A Rust compiler validates the data, and UE PCG plus Python generation produce road meshes, intersections, curbs, markings, sidewalks and lots. Buildings are assembled from modular kits (AI-generated façade/trim modules, plus Fab/Megascans assets where licensing permits) by rules per district, and hero landmarks per district (stadium, bridge, observation tower) are generated individually and placed by hand-written placement data. The track corridor gets a dedicated decoration pass, while areas outside it get lower-cost HLOD dressing.

**Audio:** engine sounds from synthesis + generated samples, impacts and ambience from generative audio, music from licensed/generated tracks with stems for dynamic intensity.

## 8. Making it AI-buildable

This section is what makes the rest feasible without humans at the editor.

| Need | Solution |
|---|---|
| No GUI dependency | All assets created/modified via UE Python editor scripts and commandlets run with `UnrealEditor-Cmd`. Levels built by generation scripts from text data, never hand-edited, so `.umap` files are build outputs regenerable from the repo. |
| Diffable sources | Gameplay in C++. Tuning in TOML/JSON → DataTables/DataAssets via import scripts. Blueprint use limited to thin, generated bindings. |
| Fast inner loop | `racer-dynamics` and the backend iterate with `cargo test` in seconds. Track data validated with the Rust CLI before UE ever loads it. |
| The agent can see | A capture harness runs a packaged build headlessly on a GPU CI runner, flies scripted cameras over each track at each time/weather, and saves screenshots + Unreal Insights/CSV profiler output. Visual review by a multimodal model compares results against reference mood boards and flags regressions (golden-image diffs with tolerance). |
| The agent can feel physics | Bot drivers run every track automatically with telemetry export (speed, slip angles, g-forces, lap times). Plots are generated as images for review. |
| Netcode is testable | Automated multi-client tests: one server + 8 headless bot clients with injected latency, jitter and loss (UE network emulation). Gates measure correction magnitude, desync frequency and collision outcome agreement. |
| Builds | GitHub Actions with self-hosted GPU runners (Windows + Linux, plus a Mac runner) building client, server and tests via RunUAT BuildCookRun. Large binaries in Git LFS. |

## 9. Repository layout

```
racer/
  game/                  UE project (Source/ C++, Config/, generated Content/)
    Source/Racer/        gameplay, vehicles, race rules, UI, netcode glue
    Source/RacerDynamicsFFI/   ThirdParty module linking the Rust static lib
  crates/
    racer-dynamics/      vehicle physics core (Rust, no_std-friendly, cxx bridge)
    racer-trackc/        city/track data compiler + validator
    racer-backend/       matchmaking, lobbies, leaderboards, fleet API
  data/
    cars/<id>/           car.toml, brief.md, livery definitions
    city/                roads.json, districts/*.toml
    tracks/<id>/         track.toml, AI racing lines (generated)
  pipelines/
    blender/             headless car/prop processing scripts
    unreal/              editor Python: import, level generation, capture
  tools/                 CI scripts, capture/review harness
  docs/                  design docs, art direction mood boards
```

## 10. Milestones

Each milestone ends with a playable, automatically verified build. Exit criteria are checked by CI, not by opinion.

| # | Milestone | Scope | Exit criteria |
|---|---|---|---|
| M0 | Foundations | UE project, CI on all 3 OSes, Rust↔UE FFI, capture harness, Git LFS | A packaged empty level builds and runs headless on Win/Linux/Mac. A Rust function is called from C++ in a shipping build. |
| M1 | Driving feel | `racer-dynamics` v1, one test car (greybox), flat test track + skidpad, gamepad + wheel FFB, chase camera | Reference maneuver tests pass within target bands. Physics stable at 240 Hz under fuzzing. Telemetry plots match expected vehicle behavior. |
| M2 | Single-player race loop | One greybox city track, race rules (laps, checkpoints, positions, reset, finish), AI racing-line generation + driver controller with 5 difficulties, basic HUD/menus | AI completes 100 automated laps per difficulty without getting stuck. Lap-time spread across difficulties is in the target ranges. |
| M3 | Multiplayer | Dedicated server, prediction/resimulation, remote car smoothing, contact, lobbies (direct connect first), backend matchmaking v1 | An 8-bot networked race at 150 ms/1% loss completes with corrections under thresholds and car-contact outcomes agreeing between server and clients. |
| M4 | Vertical slice | Downtown district fully art-complete with 1 track, 3 hero cars through the asset pipeline, full lighting/weather/time-of-day, damage, audio | Hits performance targets on the reference hardware. Screenshot review passes the art-direction rubric. This is the go/no-go for visual quality, and the pipeline is fixed here before scaling. |
| M5 | Content scale-up | Remaining 3 districts, 12 tracks (+reverse), 16 cars, liveries, traffic, pedestrians | Every track/car combination passes automated AI lap and capture runs. Asset validators are green. |
| M6 | Online polish | Steam integration, EAC, leaderboards + ghosts, replays, private lobbies, server fleet autoscaling, settings/accessibility, remapping | A 48-hour soak test with bot lobbies passes without server crashes. Crash reporting is in place. |
| M7 | Beta → launch | Closed beta, tuning from player telemetry, performance passes per OS, store page | Crash-free rate ≥ 99.5% and performance targets met on the hardware survey's top GPUs. |

M4 is the critical gate. If AI-generated cars or the city don't reach the visual bar there, the fallback is to buy hero car models and Fab environment kits and keep the generative pipeline for long-tail props.

## 11. Main risks

| Risk | Mitigation |
|---|---|
| AI-generated hero assets fall short of the GTA VI bar (topology, panel separation, consistency) | Strict validators, M4 gate, fallback to purchased base meshes run through the same Blender pipeline. Cars are the most-viewed asset, so they get the most budget. |
| UE editor automation gaps (some operations are GUI-only) | Decide early in M0 which operations need C++ editor utilities. Keep a list and write those utilities once. |
| Physics netcode with hard ramming feels bad under latency | Prototype the netcode in M3 before art investment. Tune the extrapolation from inputs. Ghosted first lap as a safety valve. |
| macOS performance/feature parity (hardware RT, Lumen quality) | Use software Lumen on Mac from day one, keep a Mac runner in CI, and accept a tuned-down Mac preset rather than late surprises. |
| FFI boundary between Rust and UE complicates builds | Single narrow C ABI (`step(state, inputs, dt) -> forces`), prebuilt per-target static libs cached in CI, C++ port as the escape hatch. |

## 12. Open questions

Questions that don't block M0 to M2 but should be answered before M4: whether split-screen local multiplayer is wanted, whether players should be able to save and share custom liveries, whether VR (cockpit camera) support matters for the realism audience, and whether the soundtrack should be licensed music or fully generated.
