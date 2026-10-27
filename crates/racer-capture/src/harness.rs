//! Bevy plugin that flies the scene camera through scripted poses and saves a
//! screenshot at each once rendering is ready.
//!
//! "Ready" means the render world's pipeline cache has had no queued or
//! compiling pipelines for [`STABLE_FRAMES`] consecutive frames. On lavapipe,
//! shader compilation takes many seconds, so a fixed frame count captures a
//! blank clear-colour image. Each frame is additionally rejected by
//! [`crate::check::check`] and retried until it passes or the timeout expires.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{CachedPipelineState, PipelineCache},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
};
use racer_game::greybox::SceneCamera;

/// Frames the pipeline cache must stay settled before a capture.
pub const STABLE_FRAMES: u32 = 10;

/// Extra time after the deadline before the watchdog kills the process.
const WATCHDOG_GRACE: Duration = Duration::from_secs(30);

/// A named camera pose.
#[derive(Clone, Debug)]
pub struct Pose {
    pub name: String,
    pub transform: Transform,
}

/// Captures one screenshot per pose into `out_dir`, then exits.
pub struct CapturePlugin {
    pub poses: Vec<Pose>,
    pub out_dir: PathBuf,
    pub timeout: Duration,
}

/// Pipeline cache counts, written by the render world and read by the main world.
#[derive(Resource, Clone, Default)]
struct PipelineStatus(Arc<[AtomicUsize; 3]>);

impl PipelineStatus {
    /// (total, pending, failed)
    fn get(&self) -> (usize, usize, usize) {
        let [t, p, f] = &*self.0;
        (
            t.load(Ordering::Relaxed),
            p.load(Ordering::Relaxed),
            f.load(Ordering::Relaxed),
        )
    }
}

#[derive(Default)]
enum Phase {
    #[default]
    Settling,
    Waiting,
}

#[derive(Resource)]
struct CaptureRun {
    poses: Vec<Pose>,
    out_dir: PathBuf,
    deadline: Instant,
    index: usize,
    phase: Phase,
    frames_in_pose: u32,
    stable_frames: u32,
    rejected: u32,
}

/// Latest screenshot delivered by the renderer.
#[derive(Resource, Default)]
struct Delivered(Option<Image>);

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        // The deadline is checked by `drive`, which only runs while the app is
        // updating. If rendering stalls (a hung driver, a window that never
        // redraws) the main loop stops too, so a watchdog thread enforces it.
        let deadline = Instant::now() + self.timeout;
        std::thread::spawn(move || {
            std::thread::sleep(deadline + WATCHDOG_GRACE - Instant::now());
            eprintln!("racer-capture: watchdog: app stopped responding; exiting");
            std::process::exit(1);
        });
        app.insert_resource(CaptureRun {
            poses: self.poses.clone(),
            out_dir: self.out_dir.clone(),
            deadline,
            index: 0,
            phase: Phase::Settling,
            frames_in_pose: 0,
            stable_frames: 0,
            rejected: 0,
        })
        .init_resource::<Delivered>()
        .init_resource::<PipelineStatus>()
        .add_systems(Update, drive);
    }

    fn finish(&self, app: &mut App) {
        let status = app.world().resource::<PipelineStatus>().clone();
        app.get_sub_app_mut(RenderApp)
            .expect("racer-capture needs a renderer")
            .insert_resource(status)
            .add_systems(
                Render,
                publish_pipeline_status.in_set(RenderSystems::Cleanup),
            );
    }
}

fn publish_pipeline_status(cache: Res<PipelineCache>, status: Res<PipelineStatus>) {
    let (mut pending, mut failed, mut total) = (0, 0, 0);
    for pipeline in cache.pipelines() {
        total += 1;
        match pipeline.state {
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

fn drive(
    mut commands: Commands,
    mut run: ResMut<CaptureRun>,
    mut delivered: ResMut<Delivered>,
    status: Res<PipelineStatus>,
    camera: Option<Single<&mut Transform, With<SceneCamera>>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut camera) = camera else {
        error!("the scene must have exactly one SceneCamera");
        exit.write(AppExit::error());
        return;
    };
    let (total, pending, failed) = status.get();
    if Instant::now() > run.deadline {
        error!(
            "timed out at pose {}/{} (pipelines: {total} total, {pending} pending, {failed} failed; {} frames rejected)",
            run.index,
            run.poses.len(),
            run.rejected
        );
        exit.write(AppExit::error());
        return;
    }
    let Some(pose) = run.poses.get(run.index).cloned() else {
        info!(
            "captured {} poses into {}",
            run.poses.len(),
            run.out_dir.display()
        );
        exit.write(AppExit::Success);
        return;
    };

    match run.phase {
        Phase::Settling => {
            if run.frames_in_pose == 0 {
                **camera = pose.transform;
            }
            run.frames_in_pose += 1;
            let ready = total > 0 && pending == 0 && failed == 0;
            run.stable_frames = if ready { run.stable_frames + 1 } else { 0 };
            if run.stable_frames >= STABLE_FRAMES {
                commands.spawn(Screenshot::primary_window()).observe(
                    |captured: On<ScreenshotCaptured>, mut delivered: ResMut<Delivered>| {
                        delivered.0 = Some(captured.image.clone());
                    },
                );
                run.phase = Phase::Waiting;
            }
        }
        Phase::Waiting => {
            let Some(image) = delivered.0.take() else {
                return;
            };
            run.phase = Phase::Settling;
            run.stable_frames = 0;
            let rgba = match image.try_into_dynamic() {
                Ok(dynamic) => dynamic.to_rgba8(),
                Err(e) => {
                    error!("cannot convert screenshot: {e:?}");
                    exit.write(AppExit::error());
                    return;
                }
            };
            match crate::check::check(&rgba) {
                Ok(stats) => {
                    let path = run.out_dir.join(format!("{}.png", pose.name));
                    if let Err(e) = rgba.save(&path) {
                        error!("cannot write {}: {e}", path.display());
                        exit.write(AppExit::error());
                        return;
                    }
                    info!(
                        "{} after {} frames: mean luma {:.1}, stddev {:.1}",
                        path.display(),
                        run.frames_in_pose,
                        stats.mean_luma,
                        stats.luma_stddev
                    );
                    run.index += 1;
                    run.frames_in_pose = 0;
                }
                Err(reason) => {
                    run.rejected += 1;
                    warn!("rejected frame for pose {}: {reason}; retrying", pose.name);
                }
            }
        }
    }
}
