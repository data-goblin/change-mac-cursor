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

    // Read current cursor data: images, size, hotspot, frame info
    fn CGSCopyRegisteredCursorImages(
        connection: i32,
        cursor_name: *const std::ffi::c_char,
        size: *mut CGSize,
        hotspot: *mut CGPoint,
        frame_count: *mut usize,
        frame_duration: *mut f64,
        images: *mut core_foundation::array::CFArrayRef,
    ) -> i32;

    // Register cursor images globally
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

// Backup name prefix (same convention as Mousecape)
const BACKUP_PREFIX: &str = "com.cmc.backup.";

// #endregion


// #region CLI Arguments

#[derive(Parser, Debug)]
#[command(name = "cmc")]
#[command(about = "Replace macOS system cursor with a custom image")]
struct Args {
    /// Path to cursor image (PNG with transparency)
    #[arg(short, long)]
    image: Option<std::path::PathBuf>,

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

    /// Restore cursor to system default (from backup)
    #[arg(short, long)]
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


// #region Cursor Backup & Registration

/// Back up the current cursor by copying its data and re-registering
/// under a backup name. Skips if backup already exists.
fn backup_cursor(connection: i32, cursor_name: &str) -> Result<(), String> {
    let backup_name = format!("{}{}", BACKUP_PREFIX, cursor_name);
    let c_backup = CString::new(backup_name.as_str())
        .map_err(|_| "Backup name contains null byte")?;
    let c_name = CString::new(cursor_name)
        .map_err(|_| "Cursor name contains null byte")?;

    // Check if backup already exists by trying to read it
    let mut size = CGSize { width: 0.0, height: 0.0 };
    let mut hotspot = CGPoint { x: 0.0, y: 0.0 };
    let mut frame_count: usize = 0;
    let mut frame_duration: f64 = 0.0;
    let mut images: core_foundation::array::CFArrayRef = std::ptr::null();

    let backup_exists = unsafe {
        CGSCopyRegisteredCursorImages(
            connection,
            c_backup.as_ptr(),
            &mut size,
            &mut hotspot,
            &mut frame_count,
            &mut frame_duration,
            &mut images,
        )
    };

    if backup_exists == 0 && !images.is_null() {
        // Backup already exists, release and skip
        unsafe { core_foundation::base::CFRelease(images as *const std::ffi::c_void); }
        println!("Backup already exists, skipping.");
        return Ok(());
    }

    // Read the current cursor data
    let mut cur_size = CGSize { width: 0.0, height: 0.0 };
    let mut cur_hotspot = CGPoint { x: 0.0, y: 0.0 };
    let mut cur_frame_count: usize = 0;
    let mut cur_frame_duration: f64 = 0.0;
    let mut cur_images: core_foundation::array::CFArrayRef = std::ptr::null();

    let read_result = unsafe {
        CGSCopyRegisteredCursorImages(
            connection,
            c_name.as_ptr(),
            &mut cur_size,
            &mut cur_hotspot,
            &mut cur_frame_count,
            &mut cur_frame_duration,
            &mut cur_images,
        )
    };

    if read_result != 0 || cur_images.is_null() {
        return Err(format!("Failed to read current cursor data (error: {})", read_result));
    }

    // Register the current cursor data under the backup name
    let mut seed: i32 = 0;
    let reg_result = unsafe {
        CGSRegisterCursorWithImages(
            connection,
            c_backup.as_ptr(),
            true,
            true,
            cur_size,
            cur_hotspot,
            cur_frame_count,
            cur_frame_duration,
            cur_images,
            &mut seed,
        )
    };

    // Release the copied images
    unsafe { core_foundation::base::CFRelease(cur_images as *const std::ffi::c_void); }

    if reg_result != 0 {
        return Err(format!("Failed to save cursor backup (error: {})", reg_result));
    }

    println!("Backed up current cursor as '{}'", backup_name);
    Ok(())
}


/// Restore cursor from backup.
///
/// Reads the backed-up cursor data, re-registers it under the
/// original name, effectively replacing whatever custom cursor was set.
fn restore_cursor(cursor_name: &str) -> Result<(), String> {
    let connection = unsafe { CGSMainConnectionID() };
    if connection <= 0 {
        return Err("Failed to get CGS connection. Not running in a GUI session?".into());
    }

    let backup_name = format!("{}{}", BACKUP_PREFIX, cursor_name);
    let c_backup = CString::new(backup_name.as_str())
        .map_err(|_| "Backup name contains null byte")?;
    let c_name = CString::new(cursor_name)
        .map_err(|_| "Cursor name contains null byte")?;

    // Read backup cursor data
    let mut size = CGSize { width: 0.0, height: 0.0 };
    let mut hotspot = CGPoint { x: 0.0, y: 0.0 };
    let mut frame_count: usize = 0;
    let mut frame_duration: f64 = 0.0;
    let mut images: core_foundation::array::CFArrayRef = std::ptr::null();

    let read_result = unsafe {
        CGSCopyRegisteredCursorImages(
            connection,
            c_backup.as_ptr(),
            &mut size,
            &mut hotspot,
            &mut frame_count,
            &mut frame_duration,
            &mut images,
        )
    };

    if read_result != 0 || images.is_null() {
        return Err(format!(
            "No backup found for '{}'. Was the cursor changed with cmc? (Log out to reset instead.)",
            cursor_name
        ));
    }

    // Re-register under original name
    let mut seed: i32 = 0;
    let reg_result = unsafe {
        CGSRegisterCursorWithImages(
            connection,
            c_name.as_ptr(),
            true,
            true,
            size,
            hotspot,
            frame_count,
            frame_duration,
            images,
            &mut seed,
        )
    };

    // Release the copied images
    unsafe { core_foundation::base::CFRelease(images as *const std::ffi::c_void); }

    if reg_result != 0 {
        return Err(format!("Failed to restore cursor (error: {})", reg_result));
    }

    Ok(())
}


/// Register a custom cursor image with the macOS WindowServer.
///
/// Backs up the current cursor first, then replaces it.
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

    // Backup current cursor before replacing
    backup_cursor(connection, cursor_name)?;

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
        println!("Restoring cursor: {}", args.cursor);
        match restore_cursor(&args.cursor) {
            Ok(()) => {
                println!("Cursor restored to system default.");
            }
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    let image_path = match &args.image {
        Some(p) => p,
        None => {
            eprintln!("--image is required (or use --restore to reset cursors)");
            std::process::exit(1);
        }
    };

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
    println!("Loading cursor image: {}", image_path.display());
    println!("Target size: {}x{}", size, size);
    println!("Hotspot: ({}, {})", args.hotspot_x, args.hotspot_y);
    println!("Cursor ID: {}", args.cursor);

    let cg_image = match load_cursor_image(image_path, size) {
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
            println!("Use `cmc --restore` to restore the original.");
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

// #endregion
