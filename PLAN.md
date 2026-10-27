# Racer: Project Plan

A multiplayer street racing game set in a fictional city. The target is GTA V/VI visual quality or better, with realistic driving physics and real-world cars. It runs on PC (Windows, Linux, macOS), with races of up to 8 players or solo against AI. All cars and tracks are unlocked from the start and there is no progression. AI agents build it end to end with no human in the loop, and assets come from generative pipelines.

## 1. Constraints that shape everything

| Decision input | Answer | Consequence |
|---|---|---|
| Who builds it | AI agents, unattended, large token budget | Every tool must install from a script with no account, login, EULA click or GUI. Everything is text in the repo. The agent must see its own results (screenshots, telemetry, profiles) on a CPU-only Linux container. |
| Platforms | Windows, Linux, macOS (Apple Silicon) | Renderer on Vulkan, DX12 and Metal. |
| Lobby size | 8 | Server-authoritative physics with prediction is affordable. Every car is fully simulated on the server. |
| City | Fictional | No real-world map licensing. The city is designed around the tracks. |
| Cars | Real production cars | Accurate geometry and real specs are required, and shipping them needs manufacturer licenses (section 11). |

## 2. Technology choice

**Engine: Bevy (Rust), pinned to a release (0.19.1 at the time of writing).** The whole game, tools and backend are one Cargo workspace.

**Why not Unreal Engine 5:** UE5 has the best renderer, but an agent cannot set it up unattended. The engine is only available after a human links an Epic account to GitHub and accepts the EULA. The binary download sits behind an Epic login and Cloudflare challenge, and the `ghcr.io/epicgames` Docker images need that linked account's token. A source build also needs far more disk than an agent container has (≈31 GB free here), and the editor workflow is GUI- and binary-asset-centric. Godot installs unattended but its renderer ceiling is lower than Bevy's (no hardware ray-traced GI or virtual geometry), and its scene and editor workflow is less agent-friendly than plain Rust.

**Verified in this environment (no human steps):** on the 4-core, 15 GB RAM, GPU-less container, this sequence works unattended. Install `mesa-vulkan-drivers` (lavapipe software Vulkan), `xvfb`, `libxkbcommon-x11-0` and Bevy's Linux build dependencies (`pkg-config libwayland-dev libxkbcommon-dev libasound2-dev libudev-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev`), then `cargo add bevy`. A PBR scene with clearcoat material and shadow maps then builds and renders, and a screenshot written via Bevy's screenshot API is read back by the agent. 440 frames, including first-run shader compilation, took 75 s at 1280×720. That is slow but fine for automated visual checks.

**What Bevy gives and what has to be built.** Bevy is less finished than UE5, so reaching the visual bar is partly renderer engineering. The plan budgets for it (M4), and because the renderer is plain Rust and WGSL in the dependency tree, agents can extend or fork it like any other code.

| Capability | Bevy 0.19 status | Plan |
|---|---|---|
| PBR, clearcoat, anisotropy, transmission, clustered decals, PCSS soft shadows, SSR, SSAO, volumetric fog, atmosphere, bloom, TAA/SMAA, DLSS | Built in (some behind feature flags) | Use as is. Tune. |
| Real-time ray-traced GI and reflections | `bevy_solari`, experimental, needs hardware RT | Primary GI path on RT-capable GPUs. Harden and contribute fixes upstream. |
| Fallback GI for non-RT GPUs and macOS | Not built in at Lumen quality | Build: baked irradiance volumes + reflection probes per track (tracks are fixed, so baking is cheap and high quality), plus SSR/SSGI. |
| Virtual geometry (Nanite-like) | `meshlet`, experimental | Use for static city geometry if it holds up at M4. Otherwise use conventional LODs + HLOD impostors. |
| Many dynamic lights (neon, street lamps, headlights) | Clustered forward | Profile at M4. Add light culling/budgeting per view if needed. |
| Car paint, wet road, rain, puddles | Partially via extended materials | Build: custom WGSL material extensions (section 6). |
| FSR / XeSS / MetalFX | Not built in | Add as plugins if TAA + DLSS is insufficient. |

**Rest of the stack (all crates.io, versions pinned to the chosen Bevy release at M0):**

| Area | Choice |
|---|---|
| Rigid bodies and collision | Avian 3D (ECS-native, works with lightyear rollback). Rapier is the fallback. |
| Vehicle dynamics | `racer-dynamics`, our own crate (section 4) |
| Networking | lightyear (client prediction, rollback, interpolation, Avian integration), over UDP/WebTransport |
| UI | bevy_ui for the in-game HUD, egui for debug and tuning tools |
| Particles | bevy_hanabi (spray, smoke, sparks, rain) |
| Audio | Kira (via bevy_kira_audio) with custom engine-sound synthesis DSP |
| Input / force feedback | gilrs for gamepads. SDL3 (via `sdl3` crate) for steering wheels and force feedback on all three OSes. |
| Backend | axum, tokio, sqlx, Postgres |

**Platform services stay optional so the agent never blocks on an account.** The core game, its own Rust backend and dedicated servers need no third-party accounts, with direct-connect and invite-code lobbies built in. Steam (identity, overlay, store) and a hosting provider are plugged in behind traits at release. Anti-cheat relies on server authority (inputs, not positions, are trusted) instead of a kernel anti-cheat that needs a vendor account.

## 3. Game design

**Modes:** Circuit (laps), Sprint (point to point), Time Trial (solo, ghosts), and Free Roam of the track corridor for practice. Every mode is available solo vs AI and online. Online lobbies can be public matchmade or private with an invite code, and humans can be mixed with AI fill.

**Race options (all available from the start):** time of day (dawn, day, golden hour, night), weather (dry, overcast, rain, wet-drying), track direction (forward and reverse), traffic (off or light), collisions (full, ghosted for the first lap, off), damage (cosmetic, cosmetic + mechanical, off), AI difficulty, and assists (ABS, TCS, stability control, steering assist, racing line). Assists are per player and shown in the results screen so times stay comparable.

**Contact:** Ramming is first-class, and car-to-car contact is fully physical. Visual damage covers deformation, detachable bumpers, mirrors, hoods and doors, and broken glass and lights. With mechanical damage on, contact also bends alignment, cuts power and punctures tires. A car flipped or stuck for 3 s resets to the last valid track position.

**Cars (16 real production cars, 6 classes).** The list is provisional and may change based on licensing.

| Class | Cars |
|---|---|
| Hot hatch | Volkswagen Golf GTI (Mk8), Honda Civic Type R (FL5), Toyota GR Yaris |
| Sport sedan | BMW M3 Competition (G80), Audi RS 3 Sedan, Cadillac CT5-V Blackwing |
| Muscle | Ford Mustang GT (S650), Dodge Challenger SRT Hellcat, Chevrolet Camaro ZL1 |
| Sports / GT | Porsche 911 Carrera S (992), Nissan GT-R (R35), Toyota GR Supra (A90) |
| Supercar | Lamborghini Huracán EVO, McLaren 750S |
| Hypercar | Bugatti Chiron Super Sport, Koenigsegg Jesko Absolut |

Races can be class-locked or open, and each car has 8 liveries plus a color picker.

**City and tracks:** 4 districts: Downtown (glass towers, tight 90° grids, tunnels), Harbor (docks, containers, a drawbridge), Hills (switchbacks, canyon road, overlook), and Old Town/Strip (neon, narrow cobbled streets). Each has 3 track layouts, for 12 layouts and 24 with reverse, all drivable in every time and weather combination.

**AI difficulty (5 levels):** Novice, Amateur, Pro, Expert, Alien. The levels scale racing-line precision, braking-point margin, corner-speed fraction, mistake rate, overtaking aggression and contact willingness. There is no rubber-banding by default, with a lobby option for mild catch-up. AI drives the exact same physics as players.

## 4. Vehicle physics (realism core)

Avian owns the chassis rigid body (mass, inertia tensor, collisions with the world and other cars). `racer-dynamics` is a pure Rust crate with no Bevy dependency. It computes wheel, tire, suspension, drivetrain and aero forces from chassis state + inputs, and a thin Bevy plugin applies them each fixed step. The same code runs on client, server and AI, and in headless tests.

| Subsystem | Model |
|---|---|
| Rate | Fixed 240 Hz chassis step. Wheel spin and drivetrain substepped ×4 (960 Hz) inside the crate for stiff-system stability. |
| Tires | Pacejka Magic Formula combined slip, load sensitivity, camber thrust, relaxation length. Grip multipliers per surface (dry/wet asphalt, painted lines, cobbles, metal plates, grass, dirt). Tire temperature and wear are out of scope. |
| Contact | Wheel shape-casts (not single raycasts), so curbs and potholes read correctly at speed. |
| Suspension | Spring, bump/rebound damper, bump stop, anti-roll bars, camber/toe curves vs travel. |
| Drivetrain | Real torque curve, rev limiter, flywheel inertia, clutch, gearbox (manual/auto/DCT), differentials (open, LSD, e-diff approximation, AWD split including torque-vectoring approximations for cars like the GR Yaris and GT-R). |
| Aero | Drag, front/rear downforce with ride-height sensitivity, active aero where the real car has it (simplified), slipstream. |
| Assists | ABS, TCS, ESC as controllers inside the crate, so they behave identically for AI and players. |
| Input | Keyboard, gamepad (speed-sensitive steering filter with assist on), steering wheels with force feedback from the tire model's self-aligning torque. |

**Real-car data.** Each car is `data/cars/<id>/car.toml`, holding the published specs with a source citation per field: curb weight, weight distribution, dimensions, wheelbase and track, tire sizes, power and torque curves, gear and final-drive ratios, top speed, drivetrain layout, and brake sizes. Values that are never published (inertia tensor, damper curves, tire coefficients) are estimated from class norms and then fitted.

**Validation against reality.** `cargo test` runs reference maneuvers headlessly for every car: 0 to 100 km/h, quarter mile, 100 to 0 braking, skidpad lateral g, and top speed. Results must land within tolerance of the real car's published performance figures and reputable magazine test data, stored next to the specs. An agent changing a tire coefficient gets a failing test if the M3 suddenly stops in 30 m or the Chiron loses 40 km/h of top speed. A fuzz/soak suite catches NaNs, explosions and jitter at extreme inputs.

## 5. Multiplayer architecture

**Topology:** dedicated, server-authoritative headless servers (`racer-server`, the same Bevy app without rendering, built for Linux containers). One process per race, 240 Hz simulation, 60 Hz snapshots.

**Netcode (lightyear):** the local car is predicted with rollback, so on correction the client rewinds to the server's state and resimulates buffered inputs through Avian + `racer-dynamics`. Remote cars are extrapolated from the latest snapshot plus their last inputs (throttle/steer/brake predict far better than dead reckoning) and smoothed over 100 to 150 ms. Car-to-car contact is predicted locally against the extrapolated remote car and resolved authoritatively by the server, with corrections blended visually over a few frames. Target: playable to 150 ms RTT, with a warning above 250 ms.

**Fairness:** the server trusts inputs only, so speed hacks and teleports are structurally impossible. Collision griefing is mitigated by the ghosted-first-lap option and vote-kick.

**Backend (`racer-backend`, Rust):** anonymous device-key accounts (Steam auth added at release), region/mode/class matchmaking, invite-code lobbies, server allocation behind a `Fleet` trait (local process spawner for dev and tests, Kubernetes/hosting provider later), and per-track/class/assist lap-time leaderboards with ghost replays. Leaderboards rank times and are not progression.

**Replays:** input streams + periodic state keyframes, used for full replays with free camera and Time Trial ghosts.

## 6. Graphics plan

| Area | Approach |
|---|---|
| Lighting | Physical sky + atmosphere with time-of-day presets. Solari ray-traced GI and reflections on RT GPUs. Baked irradiance volumes + reflection probes per track and time-of-day preset for non-RT GPUs and macOS. PCSS cascaded sun shadows. Clustered lights for street, neon and headlights. A per-preset color-grading LUT built from reference mood boards. |
| Wet roads | A global wetness parameter drives a custom material extension: puddle masks, darkened albedo, roughness drop, ripple normals, SSR/RT reflections, plus rain, tire spray and wipers. This is the single biggest "GTA VI" visual signal and gets dedicated polish time. |
| Car paint | Custom material: base + metallic flake (procedural normal sparkle) + clearcoat, with dirt and water layers. Real glass with transmission, an interior visible through windows, emissive light clusters, brake disc glow. LOD0 around 300 to 500k triangles. |
| Damage | Vertex-shader deformation from impact-driven damage zones + detachable panels as physics bodies. Breakable props (fences, bins, signs). |
| City | Streaming per district cell, high density along the track corridor, HLOD impostors for the skyline, meshlets for static geometry if stable. Non-interactive pedestrians behind barriers, optional ambient traffic. |
| Post / camera | TAA or DLSS, motion blur, bloom, exposure, DoF in replays. Chase, hood, bumper and cockpit cameras with suspension-driven shake. |
| Audio | Engine synthesis per real engine type (V8, flat-six, turbo I4, W16, …) from RPM/load-mapped grains, tire squeal driven by slip, impacts driven by impulse, reverb zones (tunnels). |

**Performance targets:** 1080p/60 High on an RTX 3060/RX 6700 class GPU, 1440p/60 Ultra with ray tracing on an RTX 4070, 4K/60 with DLSS on an RTX 4080+, and Apple M2 Pro or later at 1440p/60 High with baked GI. 16 GB RAM and an SSD are required.

## 7. Content pipeline (AI-generated, real-car accurate)

Every asset is produced by scripts whose inputs live in the repo as text, so an agent can regenerate or iterate any asset unattended. Blender runs headless (`blender -b --python …`, installable from the official tarball with no account). Assets ship to Bevy as glTF/GLB + KTX2 textures.

**Real cars.** Reference set per car (`data/cars/<id>/refs/`): orthographic blueprints, published dimensions, and multi-angle photos. Image-to-3D generation produces a base form, and the Blender pipeline then fits it to the blueprints (the length, width, height, wheelbase and track from `car.toml` are hard constraints). It retopologizes, splits the car into panels (doors, hood, bumpers, mirrors, glass, lights, wheels), unwraps UVs, builds LODs and collision hulls, and places wheel/light/exhaust sockets. Accuracy is gated automatically: the pipeline renders orthographic silhouettes from front, side, top and rear and compares them with the blueprints, requiring an IoU above a threshold. A multimodal review then compares beauty renders against reference photos for signature details (grille, light signatures, badges, wheel design). Cars failing either gate are regenerated or fixed by the pipeline, never shipped.

**City.** `data/city/roads.json` is a road graph (lanes, widths, elevation, bridges, tunnels), `data/city/districts/*.toml` holds style rules, density and prop palettes, and `data/tracks/<id>/track.toml` holds the route, checkpoints, start grid, barriers and spectator areas. The Rust tool `racer-trackc` validates this data and generates road meshes, intersections, curbs, markings and sidewalks. Buildings are assembled from AI-generated modular façade kits by district rules. District landmarks are generated individually. The track corridor gets a dedicated decoration pass, and everything beyond it gets cheaper HLOD dressing. GI bakes run as a headless job per track and time-of-day preset.

**Audio:** engine sounds synthesized per engine type and tuned against reference recordings, plus generated impacts and ambience. Music is generated, with stems for dynamic intensity.

## 8. Making it run without humans

| Need | Solution |
|---|---|
| Unattended setup | `tools/setup.sh` installs everything with apt and cargo (the list in section 2) on a fresh Ubuntu container, plus headless Blender. The same script runs in CI and in agent sessions. No accounts, keys or GUIs. |
| Diffable sources | Everything is Rust, WGSL, TOML/JSON/RON and Python Blender scripts. Generated binaries (GLB, KTX2, baked GI) are build outputs cached by content hash and regenerable from the repo. |
| Fast inner loop | `cargo test` on `racer-dynamics`, `racer-trackc` and the backend runs in seconds with no rendering. |
| The agent can see | `racer-capture` runs the game under Xvfb + lavapipe, flies scripted cameras over each track at each time/weather, and saves screenshots. A multimodal model reviews them against mood boards, and golden-image diffs catch regressions. CI also runs captures on GitHub-hosted Windows (WARP software D3D12) and macOS runners. |
| The agent can feel physics | Bots drive every car on every track headlessly and export telemetry (speed, slip angles, g-forces, lap times), plotted to images for review. |
| Netcode is testable | One server + 8 headless bot clients in one process tree, with injected latency, jitter and loss. Gates on correction magnitude, desync frequency and collision outcome agreement. |
| Performance | Software rendering can't measure GPU performance, so CPU frame time, draw calls, triangle and light counts are budgeted and gated in CI. If GPU hardware ever becomes available (a credential for a cloud GPU runner), it adds hardware-RT captures and real GPU timing. Every other step works without it. |
| Builds | GitHub Actions on hosted runners for Linux, Windows and macOS: client, server, tests and captures. |

Release-time steps that are inherently human (signing up for Steam, Apple notarization, Windows code-signing certificates, hosting billing, car manufacturer licensing) are kept out of the build and test loop. The game is fully buildable, testable and playable (including online via self-hosted servers) without any of them.

## 9. Repository layout

```
racer/
  Cargo.toml               workspace
  crates/
    racer-game/            Bevy client app (rendering, UI, audio, input)
    racer-sim/             shared gameplay: race rules, vehicles plugin, AI drivers, netcode protocol
    racer-dynamics/        vehicle physics core (pure Rust, no Bevy)
    racer-render/          custom materials (car paint, wet road), GI baking, post
    racer-server/          headless dedicated server
    racer-backend/         matchmaking, lobbies, leaderboards, fleet
    racer-trackc/          city/track compiler + validator
    racer-capture/         screenshot/telemetry harness for agents and CI
  assets/                  generated runtime assets (LFS / cache)
  data/
    cars/<id>/             car.toml (specs + sources), refs/, liveries
    city/                  roads.json, districts/*.toml
    tracks/<id>/           track.toml, generated racing lines
  pipelines/blender/       headless car and prop processing
  tools/                   setup.sh, CI scripts
  docs/                    design docs, art-direction mood boards
```

## 10. Milestones

Each milestone ends with a playable build verified automatically by CI.

| # | Milestone | Scope | Exit criteria |
|---|---|---|---|
| M0 | Foundations | Workspace, `tools/setup.sh`, CI on 3 OSes, `racer-capture` with Xvfb/lavapipe, crate versions pinned | A fresh container goes from `setup.sh` to a rendered screenshot with no human steps. Builds pass on Win/Linux/Mac. |
| M1 | Driving feel | `racer-dynamics` v1, Avian integration, 3 greybox cars with real specs (GTI, M3, 911), skidpad + test track, gamepad + wheel FFB, chase camera | Reference-maneuver tests match real-car data within tolerance. Stable at 240 Hz under fuzzing. |
| M2 | Single-player race loop | One greybox city track, race rules, AI racing-line generation + driver with 5 difficulties, basic HUD/menus | AI completes 100 automated laps per difficulty without getting stuck. Lap-time spread across difficulties is in the target ranges. |
| M3 | Multiplayer | Dedicated server, lightyear prediction/rollback, contact, invite lobbies, backend matchmaking v1 | An 8-bot race at 150 ms/1% loss completes with corrections under thresholds and contact outcomes agreeing. |
| M4 | Vertical slice | Downtown district art-complete with 1 track, 3 hero cars through the full pipeline, wet road, car paint, baked GI + Solari path, time of day, damage, audio | Car accuracy gates pass. Screenshot review passes the art-direction rubric. CPU budgets are met. This is the go/no-go for visuals, and the renderer work found here is scheduled before scale-up. |
| M5 | Content scale-up | 3 more districts, 12 tracks (+reverse), all 16 cars, liveries, traffic, pedestrians | Every car/track combination passes automated AI laps, captures and accuracy gates. |
| M6 | Online polish | Leaderboards + ghosts, replays, server fleet autoscaling, settings, accessibility, remapping | A 48-hour bot-lobby soak test passes without crashes. Crash reporting is in place. |
| M7 | Release prep | Steam and hosting adapters, signing and notarization hooks, licensing outcomes applied to the car list | Waits on the human release steps in section 8. Everything else is green. |

## 11. Main risks

| Risk | Mitigation |
|---|---|
| Bevy's renderer falls short of the GTA VI bar | Tracks are fixed, so baked GI + reflections do much of the work that Lumen does dynamically. Renderer work is budgeted in M4 and goes upstream where possible. A sustained, verified gap at M4 is the trigger to reconsider the engine (UE5 would then need a one-time human setup). |
| Real-car licensing | Shipping real makes, models and logos commercially requires licenses from each manufacturer, and licenses commonly restrict visible damage, which conflicts with ramming. Damage intensity is configurable per car, and a car-agnostic build (badges and logos as swappable decals) keeps the game shippable if a license fails. Development itself can proceed meanwhile. |
| Image-to-3D isn't accurate enough for real cars | Blueprint fitting + silhouette IoU gates. Fallback: purchased reference-accurate base meshes run through the same pipeline. |
| No GPU in agent containers | Raster path, CPU budgets and visuals are verified on lavapipe. Hardware-RT features are compiled and unit-tested but only visually verified once a GPU runner exists. |
| Bevy and ecosystem version churn | Pin Bevy and every ecosystem crate. Upgrade once per milestone at most, with a capture-diff gate. |
| Physics rollback cost with 8 cars at 240 Hz | Profile in M3. Resimulate only the local car and contacted bodies, and fall back to 120 Hz chassis steps with substepped tires if needed. |

## 12. Open questions

Questions that don't block M0 to M3: whether split-screen local multiplayer is wanted, whether players should be able to save and share custom liveries, whether VR cockpit support matters, and whether the car list above is the right mix.
