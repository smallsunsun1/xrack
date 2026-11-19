package dev.xrack.share_test;

import android.inputmethodservice.InputMethodService;
import android.text.InputType;
import android.view.View;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;

public final class TestInputMethodService extends InputMethodService {
    private InputConnection captured;
    private TextView status;
    private TextView observed;
    private TextView editorMode;
    private LinearLayout decimalKeys;
    private LinearLayout multilineKeys;

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
        editorMode = new TextView(this);
        editorMode.setVisibility(View.GONE);
        layout.addView(editorMode);
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
        button(fourth, "Read buffer", () -> {
            InputConnection connection = getCurrentInputConnection();
            CharSequence selection = connection.getSelectedText(0);
            if (selection != null && selection.length() > 0) {
                observed.setText("Selected " + selection.length() + " chars " + selection);
            } else {
                CharSequence before = connection.getTextBeforeCursor(100, 0);
                CharSequence after = connection.getTextAfterCursor(100, 0);
                if (before == null || after == null) {
                    observed.setText("Buffer unavailable");
                } else if (after.length() == 0) {
                    observed.setText("At end with " + before.length() + " chars " + before);
                } else {
                    observed.setText("Before " + before + " after " + after);
                }
            }
        });
        decimalKeys = new LinearLayout(this);
        decimalKeys.setVisibility(View.GONE);
        layout.addView(decimalKeys);
        button(decimalKeys, "Dot decimal", () -> getCurrentInputConnection().commitText("3.756", 1));
        button(decimalKeys, "Comma decimal", () -> getCurrentInputConnection().commitText("4,25", 1));
        button(decimalKeys, "Negative", () -> getCurrentInputConnection().commitText("-2,44", 1));
        multilineKeys = new LinearLayout(this);
        multilineKeys.setVisibility(View.GONE);
        layout.addView(multilineKeys);
        button(multilineKeys, "Return key", () -> {
            InputConnection connection = getCurrentInputConnection();
            connection.sendKeyEvent(new android.view.KeyEvent(
                    android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_ENTER));
            connection.sendKeyEvent(new android.view.KeyEvent(
                    android.view.KeyEvent.ACTION_UP, android.view.KeyEvent.KEYCODE_ENTER));
        });
        button(multilineKeys, "Select inner", () -> {
            InputConnection connection = getCurrentInputConnection();
            CharSequence before = connection.getTextBeforeCursor(100, 0);
            if (before != null && before.length() > 2) {
                connection.setSelection(before.length() - 1, 1);
            }
        });
        return layout;
    }

    @Override
    public void onStartInputView(EditorInfo info, boolean restarting) {
        super.onStartInputView(info, restarting);
        int keyboardType = info.inputType & (InputType.TYPE_MASK_CLASS | InputType.TYPE_MASK_VARIATION);
        boolean decimal = keyboardType == InputType.TYPE_CLASS_NUMBER &&
                (info.inputType & InputType.TYPE_NUMBER_FLAG_DECIMAL) != 0;
        boolean signed = (info.inputType & InputType.TYPE_NUMBER_FLAG_SIGNED) != 0;
        String mode = switch (keyboardType) {
            case InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS -> "Email";
            case InputType.TYPE_CLASS_PHONE -> "Telephone";
            case InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI -> "URL";
            case InputType.TYPE_CLASS_NUMBER -> decimal ? (signed ? "Signed decimal" : "Decimal") : "Integer";
            default -> "";
        };
        String action = switch (info.imeOptions & EditorInfo.IME_MASK_ACTION) {
            case EditorInfo.IME_ACTION_NEXT -> "Next";
            case EditorInfo.IME_ACTION_DONE -> "Done";
            default -> "Other action";
        };
        editorMode.setText(mode + " keyboard " + action);
        editorMode.setVisibility(mode.isEmpty() ? View.GONE : View.VISIBLE);
        decimalKeys.setVisibility(decimal ? View.VISIBLE : View.GONE);
        boolean multiline = (info.inputType & InputType.TYPE_MASK_CLASS) == InputType.TYPE_CLASS_TEXT
                && (info.inputType & InputType.TYPE_TEXT_FLAG_MULTI_LINE) != 0;
        multilineKeys.setVisibility(multiline ? View.VISIBLE : View.GONE);
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
