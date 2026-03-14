// #region Imports

use clap::Parser;
use core_graphics::image::CGImage;
use foreign_types::ForeignType;
use image::GenericImageView;
use std::ffi::CString;
use std::path::Path;

// #endregion


// #region Private CGS API Bindings

// Private CoreGraphics Server (CGS) API declarations.
// Exported from CoreGraphics.framework but not in public headers.
// Signatures verified against Mousecape's CGSCursor.h and apply.m.
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGSMainConnectionID() -> i32;

    // Correct parameter order from Mousecape:
    // (connection, cursorName, setGlobally, instantly, size, hotspot, frameCount, frameDuration, images, seed)
    fn CGSRegisterCursorWithImages(
        connection: i32,
        cursor_name: *const std::ffi::c_char,
        set_globally: bool,
        instantly: bool,
        size: CGSize,
        hotspot: CGPoint,
        frame_count: usize,
        frame_duration: f64,
        images: core_foundation::array::CFArrayRef,
        seed: *mut i32,
    ) -> i32;
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

// #endregion


// #region CLI Arguments

#[derive(Parser, Debug)]
#[command(name = "cmc")]
#[command(about = "Replace macOS system cursor with a custom image")]
struct Args {
    /// Path to cursor image (PNG with transparency)
    #[arg(short, long)]
    image: std::path::PathBuf,

    /// Hotspot X coordinate (pixels from left)
    #[arg(long, default_value = "0")]
    hotspot_x: f64,

    /// Hotspot Y coordinate (pixels from top)
    #[arg(long, default_value = "0")]
    hotspot_y: f64,

    /// Target cursor size in pixels (image will be scaled, 4-128)
    #[arg(short, long, default_value = "32")]
    size: u32,

    /// Cursor identifier to replace
    #[arg(short, long, default_value = "com.apple.coregraphics.Arrow")]
    cursor: String,

    /// Restore the default cursor
    #[arg(short, long, default_value = "false")]
    restore: bool,
}

// #endregion


// #region Image Loading

/// Load an image file and convert it to a CGImage.
///
/// Reads the file at `path`, resizes to `target_size` x `target_size`,
/// premultiplies alpha, and creates a CGImageRef for CGSRegisterCursorWithImages.
fn load_cursor_image(path: &Path, target_size: u32) -> Result<CGImage, String> {
    let img = image::open(path).map_err(|e| format!("Failed to open image: {}", e))?;
    let resized = img.resize_exact(
        target_size,
        target_size,
        image::imageops::FilterType::Lanczos3,
    );

    let rgba = resized.to_rgba8();
    let (width, height) = resized.dimensions();
    let raw_data = rgba.into_raw();

    // Premultiply alpha (required by CoreGraphics)
    let mut premultiplied = Vec::with_capacity(raw_data.len());
    for chunk in raw_data.chunks(4) {
        let a = chunk[3];
        let af = f32::from(a) / 255.0;
        premultiplied.push((f32::from(chunk[0]) * af) as u8);
        premultiplied.push((f32::from(chunk[1]) * af) as u8);
        premultiplied.push((f32::from(chunk[2]) * af) as u8);
        premultiplied.push(a);
    }

    let color_space = core_graphics::color_space::CGColorSpace::create_device_rgb();
    let bitmap_info = core_graphics::image::CGImageAlphaInfo::CGImageAlphaPremultipliedLast as u32;
    let bytes_per_row = (width * 4) as usize;

    let provider = core_graphics::data_provider::CGDataProvider::from_buffer(
        std::sync::Arc::new(premultiplied),
    );

    let cg_image = CGImage::new(
        width as usize,
        height as usize,
        8,  // bits per component
        32, // bits per pixel
        bytes_per_row,
        &color_space,
        bitmap_info,
        &provider,
        false,
        0, // kCGRenderingIntentDefault
    );

    Ok(cg_image)
}

// #endregion


// #region Cursor Registration

/// Register a custom cursor image with the macOS WindowServer.
///
/// Uses private CGS APIs to globally replace the named cursor.
/// The replacement persists until logout/restart.
fn register_cursor(
    cursor_name: &str,
    image: &CGImage,
    size: f64,
    hotspot: CGPoint,
) -> Result<(), String> {
    let connection = unsafe { CGSMainConnectionID() };
    if connection <= 0 {
        return Err("Failed to get CGS connection. Not running in a GUI session?".into());
    }

    let c_name = CString::new(cursor_name)
        .map_err(|_| "Cursor name contains null byte")?;

    // Create CFArray with single CGImage frame
    let image_ref = image.as_ptr() as *const std::ffi::c_void;
    let images = unsafe {
        core_foundation::array::CFArrayCreate(
            std::ptr::null(),
            &image_ref,
            1,
            &core_foundation::array::kCFTypeArrayCallBacks,
        )
    };

    let cg_size = CGSize { width: size, height: size };
    let mut seed: i32 = 0;

    let result = unsafe {
        CGSRegisterCursorWithImages(
            connection,
            c_name.as_ptr(),
            true,       // set globally
            true,       // instantly
            cg_size,
            hotspot,
            1,          // frame count
            0.0,        // frame duration (static)
            images,
            &mut seed,
        )
    };

    // Release the CFArray (Create Rule)
    unsafe {
        core_foundation::base::CFRelease(images as *const std::ffi::c_void);
    }

    if result == 0 {
        Ok(())
    } else {
        Err(format!("CGSRegisterCursorWithImages failed (error code: {})", result))
    }
}

// #endregion


// #region Main

fn main() {
    let args = Args::parse();

    if args.restore {
        eprintln!("Restore not yet implemented. Log out and back in to reset cursors.");
        std::process::exit(1);
    }

    // Validate size bounds
    if args.size < 4 || args.size > 128 {
        eprintln!("Cursor size must be between 4 and 128 pixels (got {})", args.size);
        std::process::exit(1);
    }

    // Validate hotspot within bounds
    if args.hotspot_x < 0.0 || args.hotspot_x >= args.size as f64
        || args.hotspot_y < 0.0 || args.hotspot_y >= args.size as f64
    {
        eprintln!(
            "Hotspot ({}, {}) is outside cursor bounds ({}x{})",
            args.hotspot_x, args.hotspot_y, args.size, args.size
        );
        std::process::exit(1);
    }

    let size = args.size;
    println!("Loading cursor image: {}", args.image.display());
    println!("Target size: {}x{}", size, size);
    println!("Hotspot: ({}, {})", args.hotspot_x, args.hotspot_y);
    println!("Cursor ID: {}", args.cursor);

    let cg_image = match load_cursor_image(&args.image, size) {
        Ok(img) => img,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    let hotspot = CGPoint {
        x: args.hotspot_x,
        y: args.hotspot_y,
    };

    match register_cursor(&args.cursor, &cg_image, size as f64, hotspot) {
        Ok(()) => {
            println!("Cursor replaced successfully.");
            println!("Note: resets on logout. Run again to re-apply.");
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

// #endregion
