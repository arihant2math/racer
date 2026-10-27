//! Gate 6, in-engine smoke test: load the GLB with Bevy's glTF loader, render
//! it, and check the frame. Runs as `racer-assetqa smoke <glb> <out-dir>` in
//! its own process (under Xvfb + lavapipe, see [`run`]), which writes
//! `smoke.png` and `smoke.json`.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use bevy::{
    asset::RecursiveDependencyLoadState,
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{CachedPipelineState, PipelineCache},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::WindowResolution,
    winit::WinitSettings,
};
use serde::{Deserialize, Serialize};

use crate::{report::Gate, thresholds::Thresholds};

/// What the smoke process measured.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SmokeResult {
    pub loaded: bool,
    pub error: Option<String>,
    pub meshes: usize,
    pub materials: usize,
    pub frames: u32,
    pub seconds: f32,
    /// Fraction of pixels that differ from the background.
    pub coverage: f32,
    pub luma_stddev: f32,
}

const STABLE_FRAMES: u32 = 10;

#[derive(Resource, Clone, Default)]
struct PipelineStatus(Arc<[AtomicUsize; 3]>);

#[derive(Resource)]
struct Run {
    scene: Handle<Scene>,
    out_dir: PathBuf,
    start: Instant,
    deadline: Instant,
    frames: u32,
    stable: u32,
    requested: bool,
    result: SmokeResult,
}

#[derive(Resource, Default)]
struct Delivered(Option<Image>);

/// Runs the Bevy app (call from the `smoke` subcommand).
pub fn app_main(glb: &Path, out_dir: &Path, timeout: Duration) -> Result<(), String> {
    let glb = glb
        .canonicalize()
        .map_err(|e| format!("{}: {e}", glb.display()))?;
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let dir = glb.parent().unwrap().to_string_lossy().to_string();
    let file = glb.file_name().unwrap().to_string_lossy().to_string();
    let deadline = Instant::now() + timeout;
    let watchdog_out = out_dir.to_path_buf();
    std::thread::spawn(move || {
        std::thread::sleep(timeout + Duration::from_secs(30));
        let r = SmokeResult {
            error: Some("watchdog: app stopped responding".into()),
            ..default()
        };
        let _ = std::fs::write(
            watchdog_out.join("smoke.json"),
            serde_json::to_string(&r).unwrap(),
        );
        std::process::exit(1);
    });

    let mut app = App::new();
    app.insert_resource(WinitSettings::continuous())
        .insert_resource(ClearColor(Color::srgb(0.55, 0.62, 0.70)))
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "racer-assetqa smoke".into(),
                        resolution: WindowResolution::new(960, 540)
                            .with_scale_factor_override(1.0),
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: dir,
                    ..default()
                }),
        )
        .init_resource::<PipelineStatus>()
        .init_resource::<Delivered>()
        .add_systems(Startup, move |mut commands: Commands, assets: Res<AssetServer>| {
            let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(file.clone()));
            commands.spawn(SceneRoot(scene.clone()));
            commands.spawn((
                Camera3d::default(),
                Transform::from_xyz(-4.2, 2.0, -5.2).looking_at(Vec3::new(0.0, 0.6, 0.0), Vec3::Y),
            ));
            commands.spawn((
                DirectionalLight {
                    illuminance: 8000.0,
                    shadow_maps_enabled: true,
                    ..default()
                },
                Transform::from_xyz(-3.0, 6.0, -2.0).looking_at(Vec3::ZERO, Vec3::Y),
            ));
            commands.insert_resource(GlobalAmbientLight {
                brightness: 600.0,
                ..default()
            });
            commands.insert_resource(Run {
                scene,
                out_dir: PathBuf::new(),
                start: Instant::now(),
                deadline,
                frames: 0,
                stable: 0,
                requested: false,
                result: SmokeResult::default(),
            });
        })
        .add_systems(Update, drive);
    let out = out_dir.to_path_buf();
    app.add_systems(PostStartup, move |mut run: ResMut<Run>| {
        run.out_dir = out.clone();
    });
    let status = app.world().resource::<PipelineStatus>().clone();
    app.finish();
    app.get_sub_app_mut(RenderApp)
        .ok_or("no renderer")?
        .insert_resource(status)
        .add_systems(Render, publish.in_set(RenderSystems::Cleanup));
    app.run();
    Ok(())
}

fn publish(cache: Res<PipelineCache>, status: Res<PipelineStatus>) {
    let (mut pending, mut failed, mut total) = (0, 0, 0);
    for p in cache.pipelines() {
        total += 1;
        match p.state {
            CachedPipelineState::Queued | CachedPipelineState::Creating(_) => pending += 1,
            CachedPipelineState::Err(_) => failed += 1,
            CachedPipelineState::Ok(_) => {}
        }
    }
    let [t, p, f] = &*status.0;
    t.store(total, Ordering::Relaxed);
    p.store(pending, Ordering::Relaxed);
    f.store(failed, Ordering::Relaxed);
}

fn finish(run: &mut Run, exit: &mut MessageWriter<AppExit>, error: Option<String>) {
    run.result.error = error;
    run.result.frames = run.frames;
    run.result.seconds = run.start.elapsed().as_secs_f32();
    let _ = std::fs::write(
        run.out_dir.join("smoke.json"),
        serde_json::to_string_pretty(&run.result).unwrap(),
    );
    exit.write(if run.result.error.is_none() {
        AppExit::Success
    } else {
        AppExit::error()
    });
}

#[allow(clippy::too_many_arguments)]
fn drive(
    mut commands: Commands,
    run: Option<ResMut<Run>>,
    assets: Res<AssetServer>,
    status: Res<PipelineStatus>,
    mut delivered: ResMut<Delivered>,
    meshes: Query<(), With<Mesh3d>>,
    materials: Res<Assets<StandardMaterial>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut run) = run else { return };
    let run = &mut *run;
    run.frames += 1;
    if Instant::now() > run.deadline {
        let msg = format!(
            "timed out after {} frames (load state {:?})",
            run.frames,
            assets.get_recursive_dependency_load_state(&run.scene)
        );
        finish(run, &mut exit, Some(msg));
        return;
    }
    match assets.get_recursive_dependency_load_state(&run.scene) {
        Some(RecursiveDependencyLoadState::Loaded) => run.result.loaded = true,
        Some(RecursiveDependencyLoadState::Failed(e)) => {
            finish(run, &mut exit, Some(format!("load failed: {e}")));
            return;
        }
        _ => return,
    }
    run.result.meshes = meshes.iter().count();
    run.result.materials = materials.len();
    if run.result.meshes == 0 {
        return;
    }
    if let Some(image) = delivered.0.take() {
        let rgba = match image.try_into_dynamic() {
            Ok(d) => d.to_rgba8(),
            Err(e) => {
                finish(run, &mut exit, Some(format!("cannot convert screenshot: {e:?}")));
                return;
            }
        };
        let (coverage, stddev) = measure(&rgba);
        run.result.coverage = coverage;
        run.result.luma_stddev = stddev;
        let path = run.out_dir.join("smoke.png");
        let err = rgba.save(&path).err().map(|e| format!("{}: {e}", path.display()));
        finish(run, &mut exit, err);
        return;
    }
    if run.requested {
        return;
    }
    let [t, p, f] = &*status.0;
    let ready = t.load(Ordering::Relaxed) > 0
        && p.load(Ordering::Relaxed) == 0
        && f.load(Ordering::Relaxed) == 0;
    run.stable = if ready { run.stable + 1 } else { 0 };
    if run.stable >= STABLE_FRAMES {
        run.requested = true;
        commands.spawn(Screenshot::primary_window()).observe(
            |captured: On<ScreenshotCaptured>, mut delivered: ResMut<Delivered>| {
                delivered.0 = Some(captured.image.clone());
            },
        );
    }
}

/// Fraction of pixels differing from the background (the mean of the four
/// corners) by more than 12 in any channel, and the luma standard deviation.
pub fn measure(img: &image::RgbaImage) -> (f32, f32) {
    let (w, h) = img.dimensions();
    let corners = [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)];
    let mut bg = [0f32; 3];
    for (x, y) in corners {
        let p = img.get_pixel(x, y).0;
        for k in 0..3 {
            bg[k] += p[k] as f32 / 4.0;
        }
    }
    let (mut covered, mut sum, mut sum2) = (0usize, 0f64, 0f64);
    for p in img.pixels() {
        if (0..3).any(|k| (p.0[k] as f32 - bg[k]).abs() > 12.0) {
            covered += 1;
        }
        let l = (p.0[0] as f64 + p.0[1] as f64 + p.0[2] as f64) / 3.0;
        sum += l;
        sum2 += l * l;
    }
    let n = (w * h) as f64;
    let mean = sum / n;
    (
        covered as f32 / n as f32,
        ((sum2 / n - mean * mean).max(0.0)).sqrt() as f32,
    )
}

/// Runs the smoke process under `xvfb-run` on lavapipe and turns its output
/// into gate 6.
pub fn run(glb: &Path, out_dir: &Path, t: &Thresholds, timeout: Duration) -> Gate {
    let name = "In-engine smoke test (Bevy, lavapipe)";
    let mut g = Gate::new(6, name);
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return Gate::skipped(6, name, format!("cannot find own executable: {e}")),
    };
    let have_xvfb = Command::new("xvfb-run").arg("--help").output().is_ok();
    let mut cmd = if have_xvfb {
        let mut c = Command::new("xvfb-run");
        c.args(["-a", "-s", "-screen 0 1920x1080x24"]).arg(&exe);
        c
    } else {
        Command::new(&exe)
    };
    cmd.arg("smoke")
        .arg(glb)
        .arg(out_dir)
        .arg("--timeout")
        .arg(timeout.as_secs().to_string());
    if let Some(icd) = std::fs::read_dir("/usr/share/vulkan/icd.d")
        .ok()
        .and_then(|d| {
            d.flatten()
                .map(|e| e.path())
                .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("lvp_icd")))
        })
    {
        cmd.env("VK_DRIVER_FILES", &icd).env("VK_ICD_FILENAMES", &icd);
    }
    cmd.env("WGPU_BACKEND", "vulkan");
    let _ = std::fs::remove_file(out_dir.join("smoke.json"));
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return Gate::skipped(6, name, format!("cannot start smoke process: {e}")),
    };
    let log = String::from_utf8_lossy(&output.stderr).to_string()
        + &String::from_utf8_lossy(&output.stdout);
    let _ = std::fs::write(out_dir.join("smoke.log"), &log);
    let errors: Vec<&str> = log
        .lines()
        .filter(|l| l.contains(" ERROR ") || l.contains("panicked"))
        .collect();
    let warnings: Vec<&str> = log
        .lines()
        .filter(|l| l.contains(" WARN ") && (l.contains("gltf") || l.contains("wgpu")))
        .collect();
    let result: SmokeResult = std::fs::read_to_string(out_dir.join("smoke.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| SmokeResult {
            error: Some(format!("no smoke.json (exit status {})", output.status)),
            ..SmokeResult::default()
        });
    g.check(
        "loads",
        "GLB loads in Bevy (glTF loader, all dependencies)",
        match &result.error {
            None => format!(
                "loaded; {} mesh entities, {} materials, {} frames in {:.0} s",
                result.meshes, result.materials, result.frames, result.seconds
            ),
            Some(e) => e.clone(),
        },
        "loads without error",
        result.loaded && result.error.is_none() && output.status.success(),
    );
    g.check(
        "log_errors",
        "ERROR lines or panics in the Bevy log",
        if errors.is_empty() {
            "none".to_string()
        } else {
            format!("{}: {}", errors.len(), errors.iter().take(3).copied().collect::<Vec<_>>().join(" / "))
        },
        "none",
        errors.is_empty(),
    );
    if !warnings.is_empty() {
        g.info(
            "log_warnings",
            "glTF/wgpu warnings",
            format!("{}: {}", warnings.len(), warnings.iter().take(3).copied().collect::<Vec<_>>().join(" / ")),
        );
    }
    g.check(
        "non_blank",
        "frame not blank: car coverage and luma spread",
        format!("coverage {:.1} %, luma stddev {:.1}", result.coverage * 100.0, result.luma_stddev),
        format!("coverage ≥ {:.0} %, stddev ≥ 4", t.smoke_min_coverage * 100.0),
        result.coverage >= t.smoke_min_coverage && result.luma_stddev >= 4.0,
    );
    if out_dir.join("smoke.png").exists() {
        g.images.push("smoke.png".into());
    }
    g.note("Full log: `smoke.log` in the report directory.");
    g
}
