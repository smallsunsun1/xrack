package dev.xrack.platform;

import android.content.Context;
import android.text.Editable;
import android.text.Selection;
import android.text.SpannableStringBuilder;
import android.view.KeyEvent;
import android.view.View;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

final class XRackTextInput extends View {
    private final Editable value = new SpannableStringBuilder();
    private final XRackActivity activity;
    private long token;
    private long epoch;
    private long serial;
    private int inputType;
    private int action;
    private boolean active;
    private String originalSelection = "";
    private long geometryToken;
    private int imeTop = Integer.MIN_VALUE;

    XRackTextInput(XRackActivity activity) {
        super(activity);
        this.activity = activity;
        setFocusable(false);
        setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_NO);
        getViewTreeObserver().addOnGlobalLayoutListener(
                () -> publishImeGeometry(getRootWindowInsets()));
    }

    @Override
    public android.view.WindowInsets onApplyWindowInsets(android.view.WindowInsets insets) {
        publishImeGeometry(insets);
        return super.onApplyWindowInsets(insets);
    }

    private void publishImeGeometry(android.view.WindowInsets insets) {
        if (token <= 0 || insets == null) { return; }
        View root = getRootView();
        int top = -1;
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            int bottom = insets.getInsets(android.view.WindowInsets.Type.ime()).bottom;
            if (bottom > 0 && insets.isVisible(android.view.WindowInsets.Type.ime())) {
                top = Math.max(0, root.getHeight() - bottom);
            }
        } else {
            android.graphics.Rect visible = new android.graphics.Rect();
            root.getWindowVisibleDisplayFrame(visible);
            int[] origin = new int[2];
            root.getLocationOnScreen(origin);
            int bottom = root.getHeight() - (visible.bottom - origin[1]);
            int margin = Math.round(80 * getResources().getDisplayMetrics().density);
            if (bottom > insets.getStableInsetBottom() + margin) {
                top = Math.max(0, visible.bottom - origin[1]);
            }
        }
        if (geometryToken != token || imeTop != top) {
            geometryToken = token;
            imeTop = top;
            activity.imeInsets(token, top);
        }
    }

    void sync(long newToken, long newEpoch, long acknowledged, String text,
            int start, int end, int type, int imeAction, boolean enabled) {
        if (newToken < token || (newToken == token && newEpoch < epoch)) {
            return;
        }
        boolean changed = token != newToken || epoch != newEpoch;
        if (!enabled) {
            token = newToken;
            epoch = newEpoch;
            active = false;
            BaseInputConnection.removeComposingSpans(value);
            // Clearing focus alone can leave this view eligible to receive it again.
            setFocusable(false);
            manager().hideSoftInputFromWindow(getWindowToken(), 0);
            publishImeGeometry(getRootWindowInsets());
            return;
        }
        if (!changed && (acknowledged != serial || composing())) {
            return;
        }
        boolean restart = changed || inputType != type || action != imeAction;
        token = newToken;
        epoch = newEpoch;
        inputType = type;
        action = imeAction;
        active = true;
        if (changed) {
            serial = 0;
            BaseInputConnection.removeComposingSpans(value);
            originalSelection = "";
        }
        if (!value.toString().equals(text)) {
            value.replace(0, value.length(), text);
        }
        Selection.setSelection(value, Math.min(start, value.length()), Math.min(end, value.length()));
        setFocusableInTouchMode(true);
        requestFocus();
        if (restart) {
            manager().restartInput(this);
        }
        if (changed) {
            manager().showSoftInput(this, InputMethodManager.SHOW_IMPLICIT);
        }
        updateSelection();
        if (changed) {
            publishImeGeometry(getRootWindowInsets());
            requestApplyInsets();
        }
    }

    private InputMethodManager manager() {
        return (InputMethodManager) getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
    }

    @Override
    public boolean onCheckIsTextEditor() {
        return active;
    }

    @Override
    public InputConnection onCreateInputConnection(EditorInfo info) {
        if (!active) {
            return null;
        }
        info.inputType = inputType;
        info.imeOptions = action | EditorInfo.IME_FLAG_NO_EXTRACT_UI;
        info.initialSelStart = Selection.getSelectionStart(value);
        info.initialSelEnd = Selection.getSelectionEnd(value);
        return new Connection(epoch);
    }

    private boolean composing() {
        return BaseInputConnection.getComposingSpanStart(value) >= 0;
    }

    private void captureSelection() {
        if (!composing()) {
            int start = Math.max(0, Selection.getSelectionStart(value));
            int end = Math.max(0, Selection.getSelectionEnd(value));
            originalSelection = value.subSequence(Math.min(start, end), Math.max(start, end)).toString();
        }
    }

    private void publish(int kind) {
        int start = Math.max(0, Selection.getSelectionStart(value));
        int end = Math.max(0, Selection.getSelectionEnd(value));
        int composeStart = BaseInputConnection.getComposingSpanStart(value);
        int composeEnd = BaseInputConnection.getComposingSpanEnd(value);
        if (composeStart >= 0 && composeEnd >= composeStart && kind != 2) {
            String text = value.subSequence(0, composeStart).toString()
                    + originalSelection + value.subSequence(composeEnd, value.length());
            String preedit = value.subSequence(composeStart, composeEnd).toString();
            activity.editText(token, epoch, ++serial, 1, text, preedit,
                    Math.max(0, Math.min(start - composeStart, preedit.length())),
                    Math.max(0, Math.min(end - composeStart, preedit.length())),
                    composeStart, composeStart + originalSelection.length());
        } else {
            activity.editText(token, epoch, ++serial, kind, value.toString(), "", start, end, start, end);
        }
    }

    private void updateSelection() {
        manager().updateSelection(this, Selection.getSelectionStart(value), Selection.getSelectionEnd(value),
                BaseInputConnection.getComposingSpanStart(value), BaseInputConnection.getComposingSpanEnd(value));
    }

    private final class Connection extends BaseInputConnection {
        private final long generation;
        private final long editorToken;
        private int batches;

        Connection(long generation) {
            super(XRackTextInput.this, true);
            this.generation = generation;
            this.editorToken = token;
        }

        private boolean valid() {
            return active && generation == epoch && editorToken == token;
        }

        private boolean changed(boolean result) {
            if (result) {
                publish(0);
                if (batches == 0) {
                    updateSelection();
                }
            }
            return result;
        }

        @Override
        public Editable getEditable() {
            return valid() ? value : null;
        }

        @Override
        public boolean beginBatchEdit() {
            if (!valid()) { return false; }
            batches++;
            return true;
        }

        @Override
        public boolean endBatchEdit() {
            if (!valid()) { return false; }
            if (batches > 0) { batches--; }
            if (batches == 0) { updateSelection(); }
            return batches > 0;
        }

        @Override
        public boolean setComposingText(CharSequence text, int position) {
            if (!valid()) { return false; }
            captureSelection();
            return changed(super.setComposingText(text, position));
        }

        @Override
        public boolean setComposingRegion(int start, int end) {
            if (!valid()) { return false; }
            if (composing()) {
                super.finishComposingText();
                publish(0);
            }
            int first = Math.max(0, Math.min(Math.min(start, end), value.length()));
            int last = Math.max(first, Math.min(Math.max(start, end), value.length()));
            originalSelection = value.subSequence(first, last).toString();
            return changed(super.setComposingRegion(start, end));
        }

        @Override
        public boolean finishComposingText() {
            return valid() && changed(super.finishComposingText());
        }

        @Override
        public boolean commitText(CharSequence text, int position) {
            return valid() && changed(super.commitText(text, position));
        }

        @Override
        public boolean deleteSurroundingText(int before, int after) {
            return valid() && changed(super.deleteSurroundingText(before, after));
        }

        @Override
        public boolean deleteSurroundingTextInCodePoints(int before, int after) {
            return valid() && changed(super.deleteSurroundingTextInCodePoints(before, after));
        }

        @Override
        public boolean setSelection(int start, int end) {
            if (start == Selection.getSelectionStart(value) && end == Selection.getSelectionEnd(value)) {
                return valid();
            }
            return valid() && changed(super.setSelection(start, end));
        }

        @Override
        public boolean performEditorAction(int requested) {
            if (!valid()) { return false; }
            if (composing()) {
                super.finishComposingText();
                publish(0);
            }
            publish(2);
            return true;
        }

        @Override
        public boolean sendKeyEvent(KeyEvent event) {
            if (!valid()) { return false; }
            int codePoint = event.getUnicodeChar();
            if (event.isCtrlPressed() || event.isAltPressed() || event.isMetaPressed()
                    || (codePoint == 0 && event.getKeyCode() != KeyEvent.KEYCODE_DEL
                        && event.getKeyCode() != KeyEvent.KEYCODE_ENTER)) {
                return super.sendKeyEvent(event);
            }
            if (event.getAction() != KeyEvent.ACTION_DOWN) { return true; }
            if (event.getKeyCode() == KeyEvent.KEYCODE_DEL) {
                if (composing()) {
                    int first = Math.min(Selection.getSelectionStart(value), Selection.getSelectionEnd(value));
                    int last = Math.max(Selection.getSelectionStart(value), Selection.getSelectionEnd(value));
                    if (first == last && first > 0) {
                        first = Character.offsetByCodePoints(value, first, -1);
                    }
                    if (first < last) {
                        value.delete(first, last);
                        Selection.setSelection(value, first);
                    }
                    return changed(true);
                }
                return deleteSurroundingTextInCodePoints(1, 0);
            }
            if (event.getKeyCode() == KeyEvent.KEYCODE_ENTER) {
                if ((inputType & android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE) != 0) {
                    return commitText("\n", 1);
                }
                return performEditorAction(action);
            }
            return codePoint != 0 && commitText(new String(Character.toChars(codePoint)), 1);
        }
    }
}
