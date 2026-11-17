//! Continuous pixels from this fixture's private Xvfb, never a still montage.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const MEDIA_LIMIT: u64 = 8 * 1024 * 1024;
const RECORDING_LIMIT: Duration = Duration::from_secs(40);

fn captured_frames(progress: &str) -> bool {
    let field = |key: &str| {
        progress.lines().rev().find_map(|line| {
            line.strip_prefix(key)
                .and_then(|value| value.trim().parse::<u64>().ok())
        })
    };
    field("frame=").is_some_and(|count| count >= 2)
        && field("out_time_us=").is_some_and(|elapsed| elapsed > 0)
}

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait(child: &mut Child, budget: Duration) -> Result<()> {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "Native recorder failed: {status}");
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "Native recorder exceeded its deadline"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn frames(path: &Path) -> Result<u64> {
    let probe = crate::linux_fixture::output(
        Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-count_frames",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=nb_read_frames",
                "-of",
                "json",
            ])
            .arg(path),
        Duration::from_secs(10),
    )?;
    let value: Value = serde_json::from_str(&probe)?;
    value["streams"][0]["nb_read_frames"]
        .as_str()
        .context("Native recording has no decoded frame count")?
        .parse()
        .context("Invalid native recording frame count")
}

pub(super) struct Recording {
    child: OwnedChild,
    started: Instant,
    directory: PathBuf,
    stages: Vec<Value>,
}

impl Recording {
    pub(super) fn start(directory: &Path) -> Result<Self> {
        crate::linux_fixture::verify_private_display()?;
        let geometry = crate::linux_fixture::output(
            Command::new("xdotool").arg("getdisplaygeometry"),
            Duration::from_secs(5),
        )?;
        ensure!(
            geometry == "1360 860",
            "Unexpected private display: {geometry}"
        );
        let log = fs::File::create_new(directory.join("recording.log"))?;
        let progress = fs::File::create_new(directory.join("recording-progress.txt"))?;
        let child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostats",
                "-progress",
                "pipe:1",
                "-stats_period",
                "0.2",
                "-n",
                "-f",
                "x11grab",
                "-draw_mouse",
                "0",
                "-framerate",
                "6",
                "-video_size",
                "1360x860",
                "-i",
            ])
            .arg(std::env::var("DISPLAY").context("Private display missing")?)
            .args([
                "-t",
                "40",
                "-fs",
                "8388608",
                "-an",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-crf",
                "18",
                "-pix_fmt",
                "yuv420p",
                "-threads",
                "2",
            ])
            .arg(directory.join("saved-views.mp4"))
            .stdin(Stdio::piped())
            .stdout(Stdio::from(progress))
            .stderr(Stdio::from(log))
            .spawn()
            .context("Start the private native screen recorder")?;
        let mut recording = Self {
            child: OwnedChild(child),
            started: Instant::now(),
            directory: directory.to_owned(),
            stages: Vec::new(),
        };
        // Process liveness alone does not prove that X11 pixels were captured.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            recording.live()?;
            let progress = directory.join("recording-progress.txt");
            ensure!(
                fs::metadata(&progress)?.len() <= 128 * 1024,
                "Recorder progress exceeds 128 KiB"
            );
            if captured_frames(&fs::read_to_string(progress)?) {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "Native recorder did not capture its first frames"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(recording)
    }

    fn live(&mut self) -> Result<()> {
        ensure!(
            self.started.elapsed() < RECORDING_LIMIT,
            "Native recording exceeded 40 seconds"
        );
        ensure!(
            self.child.0.try_wait()?.is_none(),
            "Native recorder stopped early; inspect recording.log"
        );
        Ok(())
    }

    pub(super) fn hold(&mut self, name: &str) -> Result<()> {
        self.live()?;
        self.stages
            .push(json!({"state":name,"elapsed_ms":self.started.elapsed().as_millis()}));
        // A readable pause in the fixture, not a delay in production activation.
        std::thread::sleep(Duration::from_millis(1200));
        self.live()
    }

    pub(super) fn finish(mut self) -> Result<()> {
        self.live()?;
        self.child
            .0
            .stdin
            .take()
            .context("Recorder input closed")?
            .write_all(b"q\n")?;
        wait(&mut self.child.0, Duration::from_secs(5))?;
        let video = self.directory.join("saved-views.mp4");
        let palette = self.directory.join("recording-palette.png");
        let gif = self.directory.join("saved-views.gif");
        ensure!(
            (1..=MEDIA_LIMIT).contains(&fs::metadata(&video)?.len()),
            "Native video exceeds 8 MiB"
        );
        for command in [
            Command::new("ffmpeg")
                .args(["-hide_banner", "-loglevel", "error", "-n", "-i"])
                .arg(&video)
                .args(["-vf", "fps=6,scale=1100:-2:flags=lanczos,palettegen=stats_mode=diff", "-frames:v", "1", "-threads", "2"])
                .arg(&palette),
            Command::new("ffmpeg")
                .args(["-hide_banner", "-loglevel", "error", "-n", "-i"])
                .arg(&video)
                .arg("-i").arg(&palette)
                .args(["-lavfi", "fps=6,scale=1100:-2:flags=lanczos[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=3", "-loop", "0", "-fs", "8388608", "-threads", "2"])
                .arg(&gif),
        ] {
            let mut child = OwnedChild(command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::from(fs::OpenOptions::new().append(true).open(self.directory.join("recording.log"))?))
                .spawn()?);
            wait(&mut child.0, Duration::from_secs(25))?;
        }
        ensure!(
            (1..MEDIA_LIMIT).contains(&fs::metadata(&gif)?.len()),
            "Native GIF reached its 8 MiB output limit"
        );
        let video_frames = frames(&video)?;
        let gif_frames = frames(&gif)?;
        ensure!(
            video_frames >= 36 && video_frames.abs_diff(gif_frames) <= 1,
            "Native recording is incomplete: {video_frames} video frames, {gif_frames} GIF frames"
        );
        fs::write(
            self.directory.join("recording.json"),
            serde_json::to_vec_pretty(&json!({
                "scope":"Continuous X11 capture of the owned native host; actions use the production control API",
                "video":"saved-views.mp4", "gif":"saved-views.gif", "fps":6,
                "source_revision":std::env::var("GITHUB_SHA").ok(),
                "capture_size":[1360,860], "gif_width":1100, "stages":self.stages,
                "decoded_frames":{"video":video_frames,"gif":gif_frames},
                "not_proven":["OS pointer delivery","physical keyboard","Windows/macOS qualification","performance"]
            }))?,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_requires_captured_frames_and_a_positive_output_time() {
        for pending in [
            "",
            "frame=0\nout_time_us=0\n",
            "frame=1\nout_time_us=166667\n",
            "frame=2\nout_time_us=0\n",
            "frame=2\nout_time_us=N/A\n",
        ] {
            assert!(!captured_frames(pending), "{pending}");
        }
        assert!(captured_frames(
            "frame=2\nout_time_us=166667\nprogress=continue\n"
        ));
        assert!(captured_frames("frame=0\nout_time_us=0\nprogress=continue\nframe=3\nout_time_us=333334\nprogress=continue\n"));
    }
}
