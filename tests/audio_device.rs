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

    // Feed a second of silence and check the callback consumes it.
    //
    // Silence rather than a tone: what is being tested is that the device
    // opens and its callback drains the queue, and zeros prove that as well as
    // anything does. This used to push a second of square wave at a fifth of
    // full scale, which came out of the speakers as a beep every time the
    // suite was run — a test that makes a noise on somebody's machine is a
    // test that gets run less often.
    {
        let mut q = out.queue.lock().unwrap();
        for _ in 0..out.sample_rate as usize {
            q.push_back(0.0);
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
