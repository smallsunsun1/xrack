use super::{ModalQueue, present_panel, presenter, until_canceled};
use objc2::{
    ClassType, DeclaredClass, declare_class, msg_send_id,
    mutability::MainThreadOnly,
    rc::Retained,
    runtime::{NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_ui_kit::{
    UIAdaptivePresentationControllerDelegate, UIColor, UIColorPickerViewController,
    UIColorPickerViewControllerDelegate, UIModalPresentationStyle, UIPresentationController,
};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    sync::Arc,
};
use tokio::sync::{oneshot, watch};
use winit::window::Window;
use xrack_runtime::Color;

pub(crate) struct ColorRequest {
    pub(crate) initial: Color,
    pub(crate) show_alpha: bool,
    pub(crate) preview: Option<watch::Sender<Color>>,
    pub(crate) reply: oneshot::Sender<Result<Color, String>>,
}

impl ColorRequest {
    pub(crate) async fn present(mut self, window: Arc<Window>, queue: ModalQueue) {
        let Some(_permit) = until_canceled(self.reply.closed(), queue.lock_owned()).await else {
            return;
        };
        let result = select_color(
            &window,
            self.initial,
            self.show_alpha,
            self.preview,
            self.reply.closed(),
        )
        .await;
        if let Some(result) = result.transpose() {
            let _ = self.reply.send(result.map_err(str::to_owned));
        }
    }
}

async fn select_color(
    window: &Window,
    initial: Color,
    show_alpha: bool,
    preview: Option<watch::Sender<Color>>,
    canceled: impl Future<Output = ()>,
) -> Result<Option<Color>, &'static str> {
    let (mtm, host) = presenter(window)?;
    let (send, finished) = oneshot::channel();
    let [red, green, blue, alpha] = initial
        .0
        .to_be_bytes()
        .map(|value| f64::from(value) / 255.0);
    let preserved_alpha = (!show_alpha).then_some(alpha);
    let delegate = mtm.alloc::<ColorDelegate>().set_ivars(ColorDelegateState {
        finished: Cell::new(Some(send)),
        preview: RefCell::new(preview),
        preserved_alpha,
    });
    // SAFETY: The delegate, controller, and starting color are created on the UIKit main thread.
    let (picker, delegate) = unsafe {
        let delegate: Retained<ColorDelegate> = msg_send_id![super(delegate), init];
        let picker = UIColorPickerViewController::init(mtm.alloc());
        picker.setModalPresentationStyle(UIModalPresentationStyle::UIModalPresentationPageSheet);
        picker.setSupportsAlpha(show_alpha);
        picker.setSelectedColor(&UIColor::colorWithRed_green_blue_alpha(
            red,
            green,
            blue,
            if show_alpha { alpha } else { 1.0 },
        ));
        picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        let presentation = picker
            .presentationController()
            .ok_or("The color picker has no UIKit presentation controller.")?;
        presentation.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        (picker, delegate)
    };
    let result = present_panel(&host, &picker, until_canceled(canceled, finished)).await;
    drop(delegate);
    let Some(result) = result else {
        return Ok(None);
    };
    result.map_err(|_| "The color picker closed without a selection.")?;

    // SAFETY: UIKit maintains the immutable selected color on this main-thread controller.
    let selected = unsafe { picker.selectedColor() };
    convert_color(&selected, preserved_alpha).map(Some)
}

fn convert_color(selected: &UIColor, preserved_alpha: Option<f64>) -> Result<Color, &'static str> {
    let (mut red, mut green, mut blue, mut selected_alpha) = (0.0, 0.0, 0.0, 0.0);
    // SAFETY: The four writable CGFloat outputs are separate and valid for this call.
    let converted = unsafe {
        selected.getRed_green_blue_alpha(&mut red, &mut green, &mut blue, &mut selected_alpha)
    };
    if !converted
        || ![red, green, blue, selected_alpha]
            .into_iter()
            .all(f64::is_finite)
    {
        return Err("The selected color could not be converted to sRGB.");
    }
    let [red, green, blue] =
        [red, green, blue].map(|component| (component.clamp(0.0, 1.0) * 255.0).round() as i64);
    Ok(Color::from_rgba(
        red,
        green,
        blue,
        preserved_alpha.unwrap_or(selected_alpha),
    ))
}

struct ColorDelegateState {
    finished: Cell<Option<oneshot::Sender<()>>>,
    preview: RefCell<Option<watch::Sender<Color>>>,
    preserved_alpha: Option<f64>,
}

declare_class!(
    struct ColorDelegate;

    unsafe impl ClassType for ColorDelegate {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackColorPickerDelegate";
    }

    impl DeclaredClass for ColorDelegate {
        type Ivars = ColorDelegateState;
    }

    unsafe impl NSObjectProtocol for ColorDelegate {}

    unsafe impl UIColorPickerViewControllerDelegate for ColorDelegate {
        #[method(colorPickerViewController:didSelectColor:continuously:)]
        fn color_picker_did_select(&self, _: &UIColorPickerViewController, selected: &UIColor, _: bool) {
            let state = self.ivars();
            if let Some(preview) = state.preview.borrow().as_ref()
                && let Ok(color) = convert_color(selected, state.preserved_alpha)
            {
                preview.send_if_modified(|latest| {
                    if *latest == color {
                        return false;
                    }
                    *latest = color;
                    true
                });
            }
        }

        #[method(colorPickerViewControllerDidFinish:)]
        fn color_picker_did_finish(&self, _: &UIColorPickerViewController) {
            self.finish();
        }
    }

    unsafe impl UIAdaptivePresentationControllerDelegate for ColorDelegate {
        #[method(presentationControllerDidDismiss:)]
        fn presentation_did_dismiss(&self, _: &UIPresentationController) {
            self.finish();
        }
    }
);

impl ColorDelegate {
    fn finish(&self) {
        let state = self.ivars();
        state.preview.take();
        if let Some(send) = state.finished.take() {
            let _ = send.send(());
        }
    }
}
