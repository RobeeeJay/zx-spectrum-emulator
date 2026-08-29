//! The disk drawn as a disk: rings of tracks, the bits on them, and where a
//! sector sits on the picture.

use eframe::egui;
use zx_rustrum::disk::{Disk, FILLER};
use zx_rustrum::ui::diskface::{draw, sector_wedge};

/// Where in the picture a colour is, in the terms the drawing thinks in.
fn pixel(image: &egui::ColorImage, x: usize, y: usize) -> egui::Color32 {
    image.pixels[y * image.size[0] + x]
}

/// The colour at a fraction of the way out from the middle, straight up: the
/// natural way to ask "what is on the ring at this radius".
fn at_radius(image: &egui::ColorImage, fraction: f32) -> egui::Color32 {
    let size = image.size[0];
    let half = size as f32 / 2.0;
    let centre = size / 2;
    let up = (fraction * half) as usize;
    pixel(image, centre, centre - up)
}

/// Track 0 is the outermost ring, where it is on the disk: that is where the
/// head parks and where the directory lives, so a disk with something on it is
/// bright at the edge and dark inside.
#[test]
fn track_zero_is_drawn_at_the_edge() {
    let mut disk = Disk::blank("test");
    // Something with bits in it on track 0, and nothing anywhere else: a
    // blank disk is $E5 all through, which has bits set, so the difference
    // has to be made rather than assumed.
    for sector in &mut disk.track_mut(0, 0).unwrap().sectors {
        sector.data.fill(0xFF);
    }
    for track in 1..40u8 {
        for sector in &mut disk.track_mut(track, 0).unwrap().sectors {
            sector.data.fill(0x00);
        }
    }
    let image = draw(&disk, 512);

    // What colours are found between two radii, since a track is a few pixels
    // wide with a gap after it and picking one radius picks the gap as often
    // as not.
    let between = |from: f32, to: f32| -> Vec<egui::Color32> {
        let steps = 200;
        (0..=steps)
            .map(|i| at_radius(&image, from + (to - from) * i as f32 / steps as f32))
            .collect()
    };

    // Where track 0 is, asked of the drawing rather than worked out from
    // numbers this test would have to keep in step: the outer part of its
    // band is the data, and the inner part the gap before track 1.
    let (_, band) = sector_wedge(40, 0, 0, 9);
    let width = band.end - band.start;
    let outer = between(band.end - width * 0.6, band.end - width * 0.05);
    assert!(
        outer.contains(&egui::Color32::WHITE),
        "track 0 is at the edge and full of ones"
    );
    assert!(
        !outer.contains(&egui::Color32::BLACK),
        "and nothing behind it is drawn there"
    );

    // Further in are the tracks of noughts, and no white at all.
    let inner = between(0.40, 0.85);
    assert!(inner.contains(&egui::Color32::BLACK));
    assert!(
        !inner.contains(&egui::Color32::WHITE),
        "the tracks behind track 0 are noughts"
    );

    // Outside the disk is nothing at all, so the window behind shows through.
    assert_eq!(pixel(&image, 1, 1), egui::Color32::TRANSPARENT);

    // And the middle is the hub, not data.
    let hub = pixel(&image, 256, 256);
    assert_ne!(hub, egui::Color32::WHITE);
    assert_ne!(hub, egui::Color32::BLACK);
}

/// The bits drawn are the bits on the disk, and the same bit of every nth
/// byte rather than every nth bit.
///
/// That distinction is the whole picture. A stride that is not a whole number
/// of bytes walks through the bits of a repeating pattern and turns it into
/// noise: a track of the formatter's $E5 came out looking exactly like a track
/// of code, which is the one thing this is for telling apart.
#[test]
fn the_bits_drawn_are_the_bits_on_the_disk() {
    let count_of = |fill: u8| -> (usize, usize) {
        let mut disk = Disk::blank("test");
        for track in 0..40u8 {
            for sector in &mut disk.track_mut(track, 0).unwrap().sectors {
                sector.data.fill(fill);
            }
        }
        let image = draw(&disk, 256);
        let white = image
            .pixels
            .iter()
            .filter(|p| **p == egui::Color32::WHITE)
            .count();
        let black = image
            .pixels
            .iter()
            .filter(|p| **p == egui::Color32::BLACK)
            .count();
        (white, black)
    };

    // $FF and $E5 both have their top bit set, so both read white; $7F and
    // $00 do not, and read black. A byte the same all the way round a track
    // is a track that is one colour, which is what makes an unwritten one
    // obvious.
    for solid in [0xFFu8, 0xE5] {
        let (white, black) = count_of(solid);
        assert!(
            white > 1000 && black == 0,
            "${solid:02X} has its top bit set: {white} white, {black} black"
        );
    }
    for solid in [0x00u8, 0x7F] {
        let (white, black) = count_of(solid);
        assert!(
            black > 1000 && white == 0,
            "${solid:02X} has not: {white} white, {black} black"
        );
    }

    // Something with both in it is drawn with both.
    let mut mixed = Disk::blank("test");
    for track in 0..40u8 {
        for sector in &mut mixed.track_mut(track, 0).unwrap().sectors {
            for (i, byte) in sector.data.iter_mut().enumerate() {
                *byte = if (i / 64) % 2 == 0 { 0xFF } else { 0x00 };
            }
        }
    }
    let image = draw(&mixed, 256);
    let white = image
        .pixels
        .iter()
        .filter(|p| **p == egui::Color32::WHITE)
        .count();
    let black = image
        .pixels
        .iter()
        .filter(|p| **p == egui::Color32::BLACK)
        .count();
    assert!(
        white > 200 && black > 200,
        "a disk with both on it shows both: {white} white, {black} black"
    );
}

/// The tracks are drawn apart, with a gap between one and the next. Without it
/// forty rings run together into a field of noise.
#[test]
fn the_tracks_are_drawn_as_separate_rings() {
    let mut disk = Disk::blank("test");
    for track in 0..40u8 {
        for sector in &mut disk.track_mut(track, 0).unwrap().sectors {
            sector.data.fill(0xFF);
        }
    }
    let image = draw(&disk, 512);
    // Straight up from the middle, across the written band: white where the
    // tracks are and something else in the gaps between them, so the colour
    // changes many times rather than being one block.
    let mut changes = 0;
    let mut last = None;
    for step in 300..=470 {
        let colour = at_radius(&image, step as f32 / 500.0);
        if Some(colour) != last {
            changes += 1;
            last = Some(colour);
        }
    }
    assert!(
        changes > 20,
        "forty tracks with gaps between them should stripe: {changes} changes"
    );
}

/// A disk with fewer tracks than a full one leaves the rest of the band
/// unwritten rather than drawing rubbish there.
#[test]
fn tracks_the_disk_has_not_got_are_drawn_as_nothing() {
    let mut disk = Disk::blank("short");
    disk.tracks.retain(|t| t.track < 5);
    for track in &mut disk.tracks {
        for sector in &mut track.sectors {
            sector.data.fill(0xFF);
        }
    }
    let image = draw(&disk, 256);
    // Inside where the missing tracks would be: neither black nor white.
    let inner = at_radius(&image, 0.50);
    assert_ne!(inner, egui::Color32::WHITE);
    assert_ne!(inner, egui::Color32::BLACK);
}

/// Where a sector is on the picture: the first one starts at the top and they
/// go round clockwise, and the rings run outwards-in.
#[test]
fn a_sector_has_a_place_on_the_picture() {
    let (angles, radii) = sector_wedge(40, 0, 0, 9);
    assert!(
        (angles.start + std::f32::consts::FRAC_PI_2).abs() < 0.001,
        "the first sector starts at the top: {}",
        angles.start
    );
    assert!(angles.end > angles.start, "and goes clockwise");
    assert!(
        (angles.end - angles.start - std::f32::consts::TAU / 9.0).abs() < 0.001,
        "a ninth of the way round"
    );

    // Track 0 is outside track 39.
    let (_, outer) = sector_wedge(40, 0, 0, 9);
    let (_, inner) = sector_wedge(40, 39, 0, 9);
    assert!(
        outer.start > inner.end,
        "track 0 sits outside track 39: {outer:?} against {inner:?}"
    );

    // The sectors of a track tile it without overlapping.
    let (first, _) = sector_wedge(40, 3, 0, 9);
    let (second, _) = sector_wedge(40, 3, 1, 9);
    assert!(
        (second.start - first.end).abs() < 0.001,
        "one sector ends where the next begins"
    );
    let (last, _) = sector_wedge(40, 3, 8, 9);
    assert!(
        (last.end - angles.start - std::f32::consts::TAU).abs() < 0.001,
        "and the last one closes the circle"
    );
    assert_eq!(radii.end, outer.end);
}

/// Rasterising forty rings of bits is not a thing to do sixty times a second,
/// so the picture is made again only when the disk or the size changes.
///
/// The disk's revision is what says it changed: bumped when a sector is
/// written, so nothing has to compare a hundred and eighty kilobytes to find
/// out.
#[test]
fn the_picture_is_only_drawn_again_when_the_disk_changes() {
    use zx_rustrum::fdc::{Drive, Fdc};

    let mut fdc = Fdc::new();
    fdc.drives[0] = Some(Drive::new(Disk::blank("test"), None, false));
    fdc.motor = true;
    let revision = fdc.drives[0].as_ref().unwrap().disk.revision;

    // Reading changes nothing.
    for byte in [0x46u8, 0x00, 0, 0, 0xC1, 2, 0xC1, 0x2A, 0xFF] {
        fdc.write(byte);
    }
    while fdc.status() & 0x40 != 0 {
        fdc.read();
    }
    assert_eq!(
        fdc.drives[0].as_ref().unwrap().disk.revision,
        revision,
        "a read leaves the disk as it was"
    );

    // Writing does.
    for byte in [0x45u8, 0x00, 0, 0, 0xC1, 2, 0xC1, 0x2A, 0xFF] {
        fdc.write(byte);
    }
    for i in 0..512 {
        fdc.write(i as u8);
    }
    while fdc.status() & 0x40 != 0 {
        fdc.read();
    }
    assert!(
        fdc.drives[0].as_ref().unwrap().disk.revision > revision,
        "a write means the picture is out of date"
    );
}
