fn main() -> Result<(), String> {
    xrack_platform_winit::run(capture_attachments_ui::CaptureAttachmentsDemo::new())
}
