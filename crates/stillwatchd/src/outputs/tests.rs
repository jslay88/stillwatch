use wayland_client::protocol::wl_output::Subpixel;

use super::*;

fn mode(flags: Mode, width: i32, height: i32) -> wl_output::Event {
    wl_output::Event::Mode {
        flags: WEnum::Value(flags),
        width,
        height,
        refresh: 119_880,
    }
}

fn geometry(transform: Transform) -> wl_output::Event {
    wl_output::Event::Geometry {
        x: 0,
        y: 0,
        physical_width: 941,
        physical_height: 529,
        subpixel: WEnum::Value(Subpixel::Unknown),
        make: "ASUSTek COMPUTER INC".into(),
        model: "PG48UQ".into(),
        transform: WEnum::Value(transform),
    }
}

fn name(name: &str) -> wl_output::Event {
    wl_output::Event::Name { name: name.into() }
}

fn built(events: Vec<wl_output::Event>) -> Option<OutputInfo> {
    let mut output = OutputBuilder::default();
    for event in events {
        output.apply(event);
    }
    output.build()
}

#[test]
fn uses_the_name_and_current_mode() {
    let output = built(vec![
        geometry(Transform::Normal),
        mode(Mode::Preferred, 1920, 1080),
        mode(Mode::Current | Mode::Preferred, 3840, 2160),
        wl_output::Event::Scale { factor: 1 },
        name("HDMI-A-1"),
        wl_output::Event::Done,
    ]);
    assert_eq!(output, Some(OutputInfo::new("HDMI-A-1", 3840, 2160)));
}

#[test]
fn rotated_outputs_swap_width_and_height() {
    for transform in [
        Transform::_90,
        Transform::_270,
        Transform::Flipped90,
        Transform::Flipped270,
    ] {
        let output = built(vec![
            geometry(transform),
            mode(Mode::Current, 2560, 1440),
            name("DP-1"),
        ]);
        assert_eq!(
            output,
            Some(OutputInfo::new("DP-1", 1440, 2560)),
            "{transform:?}"
        );
    }
    let flipped = built(vec![
        geometry(Transform::Flipped180),
        mode(Mode::Current, 2560, 1440),
        name("DP-1"),
    ]);
    assert_eq!(flipped, Some(OutputInfo::new("DP-1", 2560, 1440)));
}

#[test]
fn incomplete_outputs_are_skipped() {
    assert_eq!(built(vec![mode(Mode::Current, 1920, 1080)]), None);
    assert_eq!(built(vec![name("DP-2")]), None);
    assert_eq!(
        built(vec![name("DP-2"), mode(Mode::Preferred, 1920, 1080)]),
        None
    );
    assert_eq!(
        built(vec![name("DP-2"), mode(Mode::Current, -1, 1080)]),
        None
    );
}

#[test]
fn finish_keeps_complete_outputs_in_order() {
    let mut first = OutputBuilder::default();
    first.apply(name("DP-1"));
    first.apply(mode(Mode::Current, 1920, 1080));
    let mut second = OutputBuilder::default();
    second.apply(name("HDMI-A-1"));
    second.apply(mode(Mode::Current, 3840, 2160));
    let listing = Listing {
        outputs: vec![first, OutputBuilder::default(), second],
        unnamed: 1,
        done: true,
    };
    assert_eq!(
        listing.finish(),
        [
            OutputInfo::new("DP-1", 1920, 1080),
            OutputInfo::new("HDMI-A-1", 3840, 2160)
        ]
    );
}
