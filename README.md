# wincap-rs

> **Disclaimer** I am still a student and new to both Rust and the Windows API and this is still in early development as I'm working on multiple other things, so just keep that in mind. The basics like screen capture should work, and I will try and implement screen recording in the coming months hopefully. PRs are welcomed!

This is a Rust library which uses the windows API to record the screen or take screenshots. It uses [Windows Graphics Capture](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture?view=winrt-22621) as opposed to older methods which where generally alot less reliable. This isn't really a full fledged package yet but I thought as I am using this in another project I might aswell make it a standalone thing.

## Basic usage

A basic example of just taking a screenshot of the whole monitor would be:

```rs
use wincap::{error, monitor};

fn take_sc() -> error::Result<()> {
    match monitor::monitor_sc(
        None,
        &wincap::ImageMode::NoSave,
    ) {
        Ok(dynamic_image) => {
            dynamic_image.save("screenshot.png").unwrap();
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn main() {
    match take_sc() {
        Ok(_) => (),
        Err(error) => {
            println!("{}", error::err_to_string(&error));
        }
    }
}
```

Alternatively you could also take a screenshot of just a specific window

```rs
fn take_sc() -> error::Result<()> {
    match window::window_sc(
        "Settings",
        None,
        &wincap::ImageMode::No,
    ) {
        Ok(dynamic_image) => {
            dynamic_image.save("screenshot.png").unwrap();
            Ok(())
        }
        Err(error) => Err(error),
    }
}
```

There is also `Save` and `NoSave` mode, both yield you a `DynamicImage` but `NoSave` does this directly without saving to disk.
