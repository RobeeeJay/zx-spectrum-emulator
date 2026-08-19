//! Laying a turn of the loop out against the frames it ran in.

use zx_rustrum::observe::Step;
use zx_rustrum::timeline::{display_window, lay_out};

const FRAME_T: u32 = 69888;

fn step(frame: u32, t: u32, entry: u16, depth: u8, enter: bool) -> Step {
    Step {
        frame,
        t,
        entry,
        depth,
        enter,
    }
}

/// Each routine gets a lane, in the order it first ran, and each call becomes
/// a bar from going in to coming out.
#[test]
fn every_call_becomes_a_bar_on_its_routines_lane() {
    // main runs the whole frame; draw runs inside it, twice.
    let steps = [
        step(0, 100, 0x8000, 1, true),
        step(0, 200, 0x8100, 2, true),
        step(0, 900, 0x8100, 2, false),
        step(0, 1000, 0x8100, 2, true),
        step(0, 1500, 0x8100, 2, false),
        step(0, 2000, 0x8000, 1, false),
    ];
    let laid = lay_out(&steps, FRAME_T);

    assert_eq!(
        laid.lanes,
        vec![0x8100, 0x8000],
        "a lane each, as they ended"
    );
    assert_eq!(laid.bars.len(), 3, "one bar per call");
    let main = laid.bars.iter().find(|b| b.entry == 0x8000).unwrap();
    assert_eq!((main.from, main.to), (100, 2000), "main spans the turn");
    let draws: Vec<&zx_rustrum::timeline::Bar> =
        laid.bars.iter().filter(|b| b.entry == 0x8100).collect();
    assert_eq!(draws.len(), 2, "draw ran twice");
    assert_eq!((draws[0].from, draws[0].to), (200, 900));
    assert_eq!((draws[1].from, draws[1].to), (1000, 1500));
    assert_eq!(draws[0].lane, draws[1].lane, "on the same lane both times");
}

/// A turn can run over several frames — Manic Miner's takes four — so the
/// times are global rather than within a frame, and a bar can cross a frame
/// boundary.
#[test]
fn a_turn_that_runs_over_a_frame_boundary_is_laid_out_end_to_end() {
    let steps = [
        step(0, 60_000, 0x9000, 1, true),
        step(1, 10_000, 0x9000, 1, false),
    ];
    let laid = lay_out(&steps, FRAME_T);
    let bar = laid.bars.first().expect("one call");
    assert_eq!(bar.from, 60_000);
    assert_eq!(
        bar.to,
        FRAME_T as u64 + 10_000,
        "a call carries on into the next frame rather than starting again"
    );
    assert_eq!(laid.span(), bar.to - bar.from);
}

/// A routine still running when the turn ends is drawn as far as the turn
/// goes: what the picture should show is something that was still going,
/// rather than nothing at all for want of a matching exit.
#[test]
fn a_call_still_running_at_the_end_is_still_drawn() {
    let steps = [
        step(0, 100, 0x8000, 1, true),
        step(0, 500, 0x8100, 2, true),
        step(0, 900, 0x8100, 2, false),
    ];
    let laid = lay_out(&steps, FRAME_T);
    let main = laid
        .bars
        .iter()
        .find(|b| b.entry == 0x8000)
        .expect("main was never left, and should still be drawn");
    assert_eq!((main.from, main.to), (100, 900), "as far as the turn goes");
}

/// A routine can be inside itself. An exit belongs to the innermost call of
/// that routine, or the bars would nest the wrong way round.
#[test]
fn a_routine_inside_itself_pairs_up_innermost_first() {
    let steps = [
        step(0, 100, 0x8000, 1, true),
        step(0, 200, 0x8000, 2, true),
        step(0, 300, 0x8000, 2, false),
        step(0, 400, 0x8000, 1, false),
    ];
    let laid = lay_out(&steps, FRAME_T);
    assert_eq!(laid.lanes, vec![0x8000], "one routine, one lane");
    let mut spans: Vec<(u64, u64)> = laid.bars.iter().map(|b| (b.from, b.to)).collect();
    spans.sort_unstable();
    assert_eq!(
        spans,
        vec![(100, 400), (200, 300)],
        "the inner call is inside the outer one"
    );
}

/// The shaded band is where the ULA is drawing the picture: a write inside it
/// may already be too late to be seen this time round.
#[test]
fn the_display_window_is_where_the_beam_is() {
    let (start, end) = display_window(14335, 192, 224);
    assert_eq!(start, 14335, "the first pixel");
    assert_eq!(end, 14335 + 192 * 224, "and 192 lines of them");
    assert!(
        end < FRAME_T,
        "which leaves the bottom border and the retrace after it"
    );
}

/// Nothing watched is an empty picture rather than a panic.
#[test]
fn nothing_watched_lays_out_to_nothing() {
    let laid = lay_out(&[], FRAME_T);
    assert!(laid.bars.is_empty() && laid.lanes.is_empty());
    assert_eq!(laid.span(), 1, "and a span that can still be divided by");
}
