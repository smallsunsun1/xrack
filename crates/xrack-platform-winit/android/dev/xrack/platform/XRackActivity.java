package dev.xrack.platform;

import android.app.NativeActivity;
import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.DialogInterface;
import android.content.Intent;

public class XRackActivity extends NativeActivity {
    private static final int PICK_FILES = 24171;
    private static boolean shareBridgeReady;
    private long pending;
    private boolean pickingDocuments;
    private PendingIntent shareCallback;
    private XRackTextInput textInput;
    private XRackAccessibility accessibility;

    native void editText(long token, long epoch, long serial, int kind,
            String value, String preedit, int start, int end, int anchorStart, int anchorEnd);
    native void imeInsets(long token, int top);
    native void accessibilityEnabled(long token, boolean enabled);
    native boolean accessibilityAction(long token, int node, int action, String text, float progress);
    native boolean accessibilitySetSelection(long token, int node, String text, int anchor, int focus);
    native boolean accessibilityTraverseText(long token, int node, String text, int anchor, int focus,
            int granularity, boolean forward, boolean extend);

    private void mountAccessibility(long token) {
        runOnUiThread(() -> {
            if (isDestroyed()) { return; }
            if (accessibility != null) { removeAccessibilityView(); }
            accessibility = new XRackAccessibility(this, token);
            addContentView(accessibility, new android.widget.FrameLayout.LayoutParams(-1, -1));
        });
    }

    private void updateAccessibility(long token, int focus, int[] nodes, float[] ranges, String[] strings) {
        runOnUiThread(() -> {
            if (accessibility != null && accessibility.token == token) {
                accessibility.publish(focus, nodes, ranges, strings);
            }
        });
    }

    private void publishAccessibilityTraversal(long token, int node, int start, int end, int granularity, boolean forward) {
        runOnUiThread(() -> {
            if (accessibility != null && accessibility.token == token) {
                accessibility.publishTraversal(node, start, end, granularity, forward);
            }
        });
    }

    private void unmountAccessibility(long token) {
        runOnUiThread(() -> {
            if (accessibility != null && accessibility.token == token) { removeAccessibilityView(); }
        });
    }

    private void removeAccessibilityView() {
        if (accessibility.getParent() instanceof android.view.ViewGroup parent) {
            parent.removeView(accessibility);
        }
        accessibility = null;
    }

    private void syncTextInput(long token, long epoch, long serial, String value,
            int start, int end, int type, int action, boolean active) {
        runOnUiThread(() -> {
            if (isDestroyed()) { return; }
            if (textInput == null) {
                textInput = new XRackTextInput(this);
                addContentView(textInput, new android.widget.FrameLayout.LayoutParams(1, 1));
            }
            textInput.sync(token, epoch, serial, value, start, end, type, action, active);
        });
    }

    private native void finishDialog(long token, int result, String payload);
    private native void finishDocumentPicker(long token, int result, Intent data);
    private native boolean launchMessage(long token, String title, String text,
            MessageListener listener, int yesLabel, int noLabel);
    private native void finishMessage(long token, int button);
    private native boolean launchShare(long token, String text);
    private native boolean shareFocusChanged(long token, boolean hasFocus);
    private static native void shareSelected(Intent result);

    private void startDialog(long token, Runnable launch) {
        runOnUiThread(() -> {
            if (isFinishing() || isDestroyed() || pending != 0) {
                finishDialog(token, -1, "The Android activity cannot open another dialog.");
                return;
            }
            pending = token;
            try {
                launch.run();
            } catch (RuntimeException error) {
                complete(token, -1, error.toString());
            }
        });
    }

    private void showDocumentPicker(long token, Intent intent) {
        startDialog(token, () -> {
            pickingDocuments = true;
            startActivityForResult(intent, PICK_FILES);
        });
    }

    private void showShareDialog(long token, String text) {
        startDialog(token, () -> {
            if (!launchShare(token, text)) {
                complete(token, -1, "The Android share request was closed.");
            }
        });
    }

    private void showMessageDialog(long token, String title, String text) {
        startDialog(token, () -> {
            if (!launchMessage(token, title, text, new MessageListener(token),
                    R.string.xrack_dialog_yes, R.string.xrack_dialog_no)) {
                complete(token, -1, "The Android message request was closed.");
            }
        });
    }

    private void startShareChooser(Intent chooser, PendingIntent callback) {
        shareBridgeReady = true;
        shareCallback = callback;
        startActivity(chooser);
    }

    public static final class ShareResultReceiver extends BroadcastReceiver {
        @Override
        public void onReceive(Context context, Intent result) {
            // A pending broadcast can start a fresh process before native
            // methods are registered.
            if (shareBridgeReady) {
                shareSelected(result);
            }
        }
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (shareCallback != null && shareFocusChanged(pending, hasFocus)) {
            complete(pending, 0, "");
        }
    }

    private final class MessageListener implements DialogInterface.OnClickListener,
            DialogInterface.OnCancelListener {
        private final long token;

        MessageListener(long token) {
            this.token = token;
        }

        @Override
        public void onClick(DialogInterface dialog, int which) {
            if (releaseDialog(token)) {
                finishMessage(token, which);
            }
        }

        @Override
        public void onCancel(DialogInterface dialog) {
            onClick(dialog, 0);
        }
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request == PICK_FILES && pickingDocuments) {
            long token = pending;
            pending = 0;
            pickingDocuments = false;
            finishDocumentPicker(token, result, data);
        }
    }

    private void complete(long token, int result, String payload) {
        if (releaseDialog(token)) {
            finishDialog(token, result, payload);
        }
    }

    private boolean releaseDialog(long token) {
        if (pending != token || token == 0) {
            return false;
        }
        pending = 0;
        pickingDocuments = false;
        if (shareCallback != null) {
            shareCallback.cancel();
            shareCallback = null;
        }
        return true;
    }

    @Override
    protected void onDestroy() {
        complete(pending, -1, "The Android activity was closed.");
        super.onDestroy();
    }
}
