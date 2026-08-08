//! Smoke test for the host audio device. Skips itself when there is no
//! output device (headless CI), since that is not an emulator fault.

use zx_rustrum::audio_out::AudioOut;

#[test]
fn the_audio_device_opens_and_drains_samples() {
    let out = match AudioOut::start() {
        Ok(out) => out,
        Err(e) => {
            eprintln!("no audio device ({e}); skipping");
            return;
        }
    };
    eprintln!(
        "opened {} at {} Hz, {} channels",
        out.device_name, out.sample_rate, out.channels
    );
    assert!(
        (8_000.0..=768_000.0).contains(&out.sample_rate),
        "implausible sample rate {}",
        out.sample_rate
    );
    assert!(out.channels >= 1);

    // Feed a second of tone and check the callback consumes it.
    {
        let mut q = out.queue.lock().unwrap();
        for i in 0..out.sample_rate as usize {
            q.push_back(if (i / 50) % 2 == 0 { 0.2 } else { -0.2 });
        }
    }
    let before = out.queue.lock().unwrap().len();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let after = out.queue.lock().unwrap().len();
    assert!(
        after < before,
        "the audio callback never ran ({before} -> {after} samples)"
    );
}
