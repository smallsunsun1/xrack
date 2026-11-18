package dev.xrack.share_test;

import android.inputmethodservice.InputMethodService;
import android.view.View;
import android.view.inputmethod.InputConnection;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;

public final class TestInputMethodService extends InputMethodService {
    private InputConnection captured;
    private TextView status;
    private TextView observed;

    @Override
    public View onCreateInputView() {
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        layout.setBackgroundColor(0xffeeeeee);
        status = new TextView(this);
        status.setText("XRack test keyboard");
        layout.addView(status);
        observed = new TextView(this);
        layout.addView(observed);
        LinearLayout first = new LinearLayout(this);
        layout.addView(first);
        button(first, "Compose ni", () -> getCurrentInputConnection().setComposingText("ni", 1));
        button(first, "Commit Unicode", () -> getCurrentInputConnection().commitText("你好🚰", 1));
        button(first, "Delete code point", () -> getCurrentInputConnection().deleteSurroundingTextInCodePoints(1, 0));
        LinearLayout second = new LinearLayout(this);
        layout.addView(second);
        button(second, "Capture connection", () -> captured = getCurrentInputConnection());
        button(second, "Old commit", () -> { if (captured != null) { captured.commitText("stale", 1); } });
        button(second, "New line", () -> getCurrentInputConnection().commitText("\nWorld", 1));
        LinearLayout third = new LinearLayout(this);
        layout.addView(third);
        button(third, "Commit number", () -> getCurrentInputConnection().commitText("47", 1));
        button(third, "Submit", () -> getCurrentInputConnection().performEditorAction(
                getCurrentInputEditorInfo().imeOptions & android.view.inputmethod.EditorInfo.IME_MASK_ACTION));
        button(third, "Reverse select", () -> {
            CharSequence before = getCurrentInputConnection().getTextBeforeCursor(4096, 0);
            if (before != null) {
                getCurrentInputConnection().setSelection(before.length(), 0);
            }
        });
        LinearLayout fourth = new LinearLayout(this);
        layout.addView(fourth);
        button(fourth, "Backspace key", () -> {
            InputConnection connection = getCurrentInputConnection();
            connection.sendKeyEvent(new android.view.KeyEvent(
                    android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_DEL));
            observed.setText("Editor buffer " + connection.getTextBeforeCursor(100, 0));
        });
        return layout;
    }

    private void button(LinearLayout row, String text, Runnable action) {
        Button button = new Button(this);
        button.setText(text);
        button.setAllCaps(false);
        button.setOnClickListener(view -> {
            InputConnection connection = getCurrentInputConnection();
            if (connection != null) {
                action.run();
                status.setText(text + " sent");
            }
        });
        row.addView(button, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
    }
}
