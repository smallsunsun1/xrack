use objc2::{
    class, msg_send_id,
    rc::Retained,
    runtime::{Bool, NSObject},
};
use objc2_foundation::{NSDictionary, NSString, NSURL};
use std::{
    ffi::c_void,
    path::Path,
    ptr::{NonNull, from_ref, null},
};
use xrack_runtime::{Image, ImageFormat, LoadedImage};

/// A bounded photo thumbnail that can be transferred back to the UI thread.
pub struct IosPhotoThumbnail {
    png: Vec<u8>,
    size: [u32; 2],
}

impl IosPhotoThumbnail {
    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn into_image(self) -> Image {
        Image::Loaded(LoadedImage::new(ImageFormat::Png, self.png, self.size))
    }
}

pub(super) fn load(path: &Path, maximum_edge: u32) -> Result<IosPhotoThumbnail, String> {
    if !(1..=xrack_assets::MAX_IMAGE_DIMENSION).contains(&maximum_edge) {
        return Err(format!(
            "The thumbnail edge must be between 1 and {} pixels.",
            xrack_assets::MAX_IMAGE_DIMENSION
        ));
    }
    let path = path.to_str().ok_or("The photo path must be UTF-8.")?;
    objc2::rc::autoreleasepool(|_| decode(path, maximum_edge))
}

fn decode(path: &str, maximum_edge: u32) -> Result<IosPhotoThumbnail, String> {
    // SAFETY: These APIs work with file-local, owned Core Foundation objects.
    unsafe {
        let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(path), false);
        let source = OwnedCf::new(CGImageSourceCreateWithURL(from_ref(&*url).cast(), null()))
            .ok_or("ImageIO cannot read this photo.")?;
        // ImageIO supplies these immortal CFString keys, which bridge to NSString.
        let keys = [
            &*kCGImageSourceCreateThumbnailFromImageAlways.cast::<NSString>(),
            &*kCGImageSourceCreateThumbnailWithTransform.cast::<NSString>(),
            &*kCGImageSourceThumbnailMaxPixelSize.cast::<NSString>(),
        ];
        let enabled: Retained<NSObject> = msg_send_id![class!(NSNumber), numberWithBool: Bool::YES];
        let edge: Retained<NSObject> =
            msg_send_id![class!(NSNumber), numberWithUnsignedInt: maximum_edge];
        let options = NSDictionary::from_vec(&keys, vec![enabled.clone(), enabled, edge]);
        let image = OwnedCf::new(CGImageSourceCreateThumbnailAtIndex(
            source.as_ptr(),
            0,
            from_ref(&*options).cast(),
        ))
        .ok_or("ImageIO cannot decode a preview for this photo.")?;
        let size = [
            u32::try_from(CGImageGetWidth(image.as_ptr()))
                .map_err(|_| "Invalid thumbnail width.")?,
            u32::try_from(CGImageGetHeight(image.as_ptr()))
                .map_err(|_| "Invalid thumbnail height.")?,
        ];
        xrack_assets::validate_image_size(size)?;
        let output = OwnedCf::new(CFDataCreateMutable(null(), 0))
            .ok_or("Cannot allocate a photo preview.")?;
        let png_type = NSString::from_str("public.png");
        let destination = OwnedCf::new(CGImageDestinationCreateWithData(
            output.as_ptr().cast_mut(),
            from_ref(&*png_type).cast(),
            1,
            null(),
        ))
        .ok_or("Cannot create the PNG encoder for this photo.")?;
        CGImageDestinationAddImage(destination.as_ptr(), image.as_ptr(), null());
        if CGImageDestinationFinalize(destination.as_ptr()) == 0 {
            return Err("Cannot encode the photo preview.".into());
        }
        let length = usize::try_from(CFDataGetLength(output.as_ptr())).unwrap_or_default();
        let bytes = CFDataGetBytePtr(output.as_ptr());
        if length == 0 || length as u64 > xrack_assets::MAX_ASSET_BYTES || bytes.is_null() {
            return Err("The encoded photo preview is empty or too large.".into());
        }
        let png = std::slice::from_raw_parts(bytes, length).to_vec();
        Ok(IosPhotoThumbnail { png, size })
    }
}

struct OwnedCf(NonNull<c_void>);

impl OwnedCf {
    /// Wrap a non-null result from a Core Foundation Create function.
    fn new(value: *const c_void) -> Option<Self> {
        NonNull::new(value.cast_mut()).map(Self)
    }

    fn as_ptr(&self) -> *const c_void {
        self.0.as_ptr()
    }
}

impl Drop for OwnedCf {
    fn drop(&mut self) {
        // SAFETY: This instance owns one Core Foundation reference.
        unsafe { CFRelease(self.as_ptr()) };
    }
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataCreateMutable(allocator: *const c_void, capacity: isize) -> *mut c_void;
    fn CFDataGetLength(data: *const c_void) -> isize;
    fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
    fn CFRelease(value: *const c_void);
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGImageGetWidth(image: *const c_void) -> usize;
    fn CGImageGetHeight(image: *const c_void) -> usize;
}

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    static kCGImageSourceCreateThumbnailFromImageAlways: *const c_void;
    static kCGImageSourceCreateThumbnailWithTransform: *const c_void;
    static kCGImageSourceThumbnailMaxPixelSize: *const c_void;
    fn CGImageSourceCreateWithURL(url: *const c_void, options: *const c_void) -> *const c_void;
    fn CGImageSourceCreateThumbnailAtIndex(
        source: *const c_void,
        index: usize,
        options: *const c_void,
    ) -> *const c_void;
    fn CGImageDestinationCreateWithData(
        data: *mut c_void,
        image_type: *const c_void,
        count: usize,
        options: *const c_void,
    ) -> *const c_void;
    fn CGImageDestinationAddImage(
        destination: *const c_void,
        image: *const c_void,
        properties: *const c_void,
    );
    fn CGImageDestinationFinalize(destination: *const c_void) -> u8;
}
