package dev.xrack.accessibility_test;

import android.accessibilityservice.AccessibilityService;
import android.app.Activity;
import android.app.Instrumentation;
import android.app.UiAutomation;
import android.os.Bundle;
import android.os.SystemClock;
import android.view.InputDevice;
import android.view.KeyCharacterMap;
import android.view.KeyEvent;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.util.ArrayDeque;
import java.util.concurrent.TimeoutException;
import java.util.function.Predicate;

public final class ControlsProbe extends Instrumentation {
    private String scenario;
    private String packageName;
    private UiAutomation automation;

    @Override public void onCreate(Bundle arguments) {
        super.onCreate(arguments);
        scenario = arguments.getString("scenario", "controls");
        packageName = switch (scenario) {
            case "inventory" -> "dev.xrack.android_table";
            case "text", "traversal" -> "dev.xrack.android_demo";
            default -> "dev.xrack.android_controls";
        };
        start();
    }

    @Override public void onStart() {
        Bundle result = new Bundle();
        try {
            automation = getUiAutomation();
            String coverage;
            switch (scenario) {
                case "inventory" -> {
                    verifyInventory();
                    coverage = "inventory collections, frozen cells, tree expansion, scroll events and stale actions";
                }
                case "text" -> {
                    verifyTextSelection();
                    coverage = "UTF-16 selection, selection events, clipboard, backspace and read-only text";
                }
                case "password" -> {
                    verifyPasswordClipboard();
                    coverage = "password clipboard permissions and reveal";
                }
                case "traversal" -> {
                    verifyTextTraversal();
                    coverage = "character and word traversal, movement events and selection extension";
                }
                default -> {
                    verify();
                    coverage = "tree, native focus, slider, switch, modal actions, expander and Set text";
                }
            }
            result.putString("stream", "\nPASS accessibility " + coverage + "\n");
            finish(Activity.RESULT_OK, result);
        } catch (AssertionError | RuntimeException | TimeoutException error) {
            StringWriter details = new StringWriter();
            error.printStackTrace(new PrintWriter(details));
            result.putString("stream", "\nFAIL " + details + "\n");
            finish(Activity.RESULT_CANCELED, result);
        }
    }

    private void verifyTextTraversal() throws TimeoutException {
        click(waitFor("Open field notes", AccessibilityNodeInfo::isClickable));
        AccessibilityNodeInfo editor = waitFor("Inspection notes", AccessibilityNodeInfo::isEditable);
        String content = "e\u0301👩‍💻\nAlpha beta";
        Bundle text = new Bundle();
        text.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, content);
        require(editor.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, text), "The notes editor rejected Set text");
        editor = waitFor("Inspection notes", node -> content.contentEquals(node.getText()));
        int character = AccessibilityNodeInfo.MOVEMENT_GRANULARITY_CHARACTER;
        int word = AccessibilityNodeInfo.MOVEMENT_GRANULARITY_WORD;
        require(editor.getMovementGranularities() == (character | word), "The editor did not expose both text granularities");
        setSelection(editor, selection(0, 0), 0, 0, content.length());
        editor = waitFor("Inspection notes", node -> selectionIs(node, 0, 0));
        require(!editor.performAction(AccessibilityNodeInfo.ACTION_PREVIOUS_AT_MOVEMENT_GRANULARITY, movement(character, false)),
                "Character traversal moved before the beginning of the document");
        traverse(editor, character, true, false, 0, 2, 2, 2);
        traverse(waitFor("Inspection notes", node -> selectionIs(node, 2, 2)), character, true, true, 2, 7, 2, 7);
        traverse(waitFor("Inspection notes", node -> selectionIs(node, 2, 7)), character, true, true, 7, 8, 2, 8);
        traverse(waitFor("Inspection notes", node -> selectionIs(node, 2, 8)), character, false, true, 7, 8, 2, 7);
        setSelection(waitFor("Inspection notes", node -> selectionIs(node, 2, 7)), selection(8, 8), 8, 8, content.length());
        traverse(waitFor("Inspection notes", node -> selectionIs(node, 8, 8)), word, true, false, 8, 13, 13, 13);
        traverse(waitFor("Inspection notes", node -> selectionIs(node, 13, 13)), word, true, true, 14, 18, 13, 18);
        editor = waitFor("Inspection notes", node -> selectionIs(node, 13, 18));
        require(!editor.performAction(AccessibilityNodeInfo.ACTION_NEXT_AT_MOVEMENT_GRANULARITY, movement(word, false)),
                "Word traversal moved past the end of the document");
        traverse(editor, word, false, true, 14, 18, 13, 14);

        click(waitFor("Read-only", node -> !node.isChecked() && node.isClickable()));
        AccessibilityNodeInfo readOnly = waitFor("Inspection notes", node -> !node.isEditable());
        setSelection(readOnly, selection(8, 8), 8, 8, content.length());
        traverse(waitFor("Inspection notes", node -> selectionIs(node, 8, 8)), word, true, false, 8, 13, 13, 13);
        waitFor("Inspection notes", node -> !node.isEditable() && content.contentEquals(node.getText()) && selectionIs(node, 13, 13));
    }

    private static Bundle movement(int granularity, boolean extend) {
        Bundle arguments = new Bundle();
        arguments.putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_MOVEMENT_GRANULARITY_INT, granularity);
        arguments.putBoolean(AccessibilityNodeInfo.ACTION_ARGUMENT_EXTEND_SELECTION_BOOLEAN, extend);
        return arguments;
    }

    private void traverse(AccessibilityNodeInfo editor, int granularity, boolean forward,
            boolean extend, int start, int end, int anchor, int focus) throws TimeoutException {
        int action = forward ? AccessibilityNodeInfo.ACTION_NEXT_AT_MOVEMENT_GRANULARITY
                : AccessibilityNodeInfo.ACTION_PREVIOUS_AT_MOVEMENT_GRANULARITY;
        AccessibilityEvent event = automation.executeAndWaitForEvent(
                () -> require(editor.performAction(action, movement(granularity, extend)), "Text traversal was rejected"),
                candidate -> candidate.getEventType() == AccessibilityEvent.TYPE_VIEW_TEXT_TRAVERSED_AT_MOVEMENT_GRANULARITY
                        && packageName.contentEquals(candidate.getPackageName())
                        && candidate.getAction() == action && candidate.getMovementGranularity() == granularity
                        && candidate.getFromIndex() == start && candidate.getToIndex() == end,
                35_000);
        require(event.getItemCount() == editor.getText().length(), "The traversal event reported the wrong UTF-16 length");
        waitFor("Inspection notes", node -> selectionIs(node, anchor, focus));
    }

    private void verifyTextSelection() throws TimeoutException {
        click(waitFor("Open field notes", AccessibilityNodeInfo::isClickable));
        AccessibilityNodeInfo editor = waitFor("Inspection notes", AccessibilityNodeInfo::isEditable);
        require(editor.isMultiLine(), "Inspection notes must expose a multiline editor");
        String content = "A你好🚰\nBZ";
        Bundle text = new Bundle();
        text.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, content);
        require(editor.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, text), "The notes editor rejected Set text");
        editor = waitFor("Inspection notes", node -> content.contentEquals(node.getText()));
        require(!editor.performAction(AccessibilityNodeInfo.ACTION_SET_SELECTION, selection(4, 5)),
                "An offset splitting the emoji's surrogate pair was accepted");
        require(!editor.performAction(AccessibilityNodeInfo.ACTION_SET_SELECTION, selection(0, 9)),
                "A selection outside the current text was accepted");
        setSelection(editor, selection(7, 1), 7, 1, content.length());
        setSelection(waitFor("Inspection notes", node -> selectionIs(node, 7, 1)), null, 1, 1, content.length());
        setSelection(waitFor("Inspection notes", node -> selectionIs(node, 1, 1)), selection(1, 7), 1, 7, content.length());
        perform(waitFor("Inspection notes", node -> hasAction(node, AccessibilityAction.ACTION_CUT)), AccessibilityAction.ACTION_CUT);
        waitFor("Inspection notes", node -> "AZ".contentEquals(node.getText()) && selectionIs(node, 1, 1));
        perform(waitFor("Inspection notes", node -> hasAction(node, AccessibilityAction.ACTION_PASTE)), AccessibilityAction.ACTION_PASTE);
        setSelection(waitFor("Inspection notes", node -> content.contentEquals(node.getText())),
                selection(1, 7), 1, 7, content.length());
        waitFor("Inspection notes", node -> selectionIs(node, 1, 7));

        long down = SystemClock.uptimeMillis();
        for (int action : new int[] {KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP}) {
            KeyEvent event = new KeyEvent(down, SystemClock.uptimeMillis(), action, KeyEvent.KEYCODE_DEL,
                    0, 0, KeyCharacterMap.VIRTUAL_KEYBOARD, 0, 0, InputDevice.SOURCE_KEYBOARD);
            require(automation.injectInputEvent(event, true), "Android rejected the Backspace event");
        }
        AccessibilityNodeInfo editable = waitFor("Inspection notes",
                node -> node.isEditable() && "AZ".contentEquals(node.getText()) && selectionIs(node, 1, 1));
        click(waitFor("Read-only", node -> !node.isChecked() && node.isClickable()));
        AccessibilityNodeInfo readOnly = waitFor("Inspection notes", node -> !node.isEditable());
        require(!readOnly.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, text), "Read-only notes accepted Set text");
        require(!editable.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, text),
                "The old editable node could still modify read-only notes");
        setSelection(readOnly, selection(0, 2), 0, 2, 2);
        readOnly = waitFor("Inspection notes", node -> "AZ".contentEquals(node.getText()) && selectionIs(node, 0, 2));
        reject(readOnly, AccessibilityAction.ACTION_CUT);
        reject(readOnly, AccessibilityAction.ACTION_PASTE);
        perform(readOnly, AccessibilityAction.ACTION_COPY);
        click(waitFor("Back to tasks", AccessibilityNodeInfo::isClickable));
        perform(waitFor("New task", AccessibilityNodeInfo::isEditable), AccessibilityAction.ACTION_FOCUS);
        waitFor("New task", AccessibilityNodeInfo::isFocused);
        require(!editable.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, text),
                "The hidden editable node could still receive Set text");
        require(!readOnly.performAction(AccessibilityNodeInfo.ACTION_SET_SELECTION, selection(0, 0)),
                "The hidden notes node could still set the selection");
        require(!readOnly.performAction(AccessibilityNodeInfo.ACTION_PREVIOUS_AT_MOVEMENT_GRANULARITY,
                        movement(AccessibilityNodeInfo.MOVEMENT_GRANULARITY_CHARACTER, false)),
                "The hidden notes node could still move the selection");
        perform(waitFor("New task", node -> hasAction(node, AccessibilityAction.ACTION_PASTE)), AccessibilityAction.ACTION_PASTE);
        waitFor("New task", node -> "AZ".contentEquals(node.getText()));
    }

    private void verifyPasswordClipboard() throws TimeoutException {
        click(waitFor("Open holds", AccessibilityNodeInfo::isClickable));
        click(waitFor("Open privacy", AccessibilityNodeInfo::isClickable));
        copyFromProbe("moss7");
        perform(waitFor("Test passphrase", AccessibilityNodeInfo::isPassword), AccessibilityAction.ACTION_FOCUS);
        perform(waitFor("Test passphrase", node -> hasAction(node, AccessibilityAction.ACTION_PASTE)), AccessibilityAction.ACTION_PASTE);
        waitFor("Bound match", node -> true);

        copyFromProbe("checkpad");
        perform(waitFor("Test passphrase", AccessibilityNodeInfo::isPassword), AccessibilityAction.ACTION_FOCUS);
        AccessibilityNodeInfo password = waitFor("Test passphrase", node -> node.isPassword() && node.isFocused());
        rejectTraversal(password);
        reject(password, AccessibilityAction.ACTION_COPY);
        reject(password, AccessibilityAction.ACTION_CUT);
        click(waitFor("Reveal value", AccessibilityNodeInfo::isClickable));
        waitFor("Mask value", AccessibilityNodeInfo::isClickable);
        perform(waitFor("Test passphrase", AccessibilityNodeInfo::isPassword), AccessibilityAction.ACTION_FOCUS);
        password = waitFor("Test passphrase", node -> node.isPassword() && node.isFocused());
        rejectTraversal(password);
        require(!"moss7".contentEquals(password.getText()), "The revealed password leaked through the accessibility node");
        reject(password, AccessibilityAction.ACTION_COPY);
        reject(password, AccessibilityAction.ACTION_CUT);
        waitFor("Bound match", node -> true);

        click(waitFor("Clear probe", AccessibilityNodeInfo::isClickable));
        waitFor("Probe empty", node -> true);
        perform(waitFor("Paste proof", AccessibilityNodeInfo::isEditable), AccessibilityAction.ACTION_FOCUS);
        perform(waitFor("Paste proof", node -> hasAction(node, AccessibilityAction.ACTION_PASTE)), AccessibilityAction.ACTION_PASTE);
        waitFor("Probe sentinel", node -> true);
    }

    private void copyFromProbe(String value) throws TimeoutException {
        AccessibilityNodeInfo probe = waitFor("Paste proof", AccessibilityNodeInfo::isEditable);
        if (!value.contentEquals(probe.getText())) {
            Bundle text = new Bundle();
            text.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, value);
            require(probe.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, text), "Clipboard probe rejected text");
        } else {
            perform(probe, AccessibilityAction.ACTION_FOCUS);
        }
        probe = waitFor("Paste proof", node -> node.isFocused() && value.contentEquals(node.getText()));
        setSelection(probe, selection(0, value.length()), 0, value.length(), value.length());
        perform(waitFor("Paste proof", node -> hasAction(node, AccessibilityAction.ACTION_COPY)), AccessibilityAction.ACTION_COPY);
    }

    private static boolean hasAction(AccessibilityNodeInfo node, AccessibilityAction action) {
        return node.getActionList().contains(action);
    }

    private static void rejectTraversal(AccessibilityNodeInfo password) {
        require(password.getMovementGranularities() == 0
                        && !password.performAction(AccessibilityNodeInfo.ACTION_NEXT_AT_MOVEMENT_GRANULARITY,
                                movement(AccessibilityNodeInfo.MOVEMENT_GRANULARITY_CHARACTER, false)),
                "The password exposed text traversal");
    }

    private static void perform(AccessibilityNodeInfo node, AccessibilityAction action) {
        require(node.performAction(action.getId()), "Action " + action.getId() + " was rejected for " + node);
    }

    private static void reject(AccessibilityNodeInfo node, AccessibilityAction action) {
        require(!hasAction(node, action) && !node.performAction(action.getId()),
                "Restricted action " + action.getId() + " was offered or accepted");
    }

    private void setSelection(AccessibilityNodeInfo editor, Bundle selection, int anchor, int focus, int length) throws TimeoutException {
        AccessibilityEvent event = automation.executeAndWaitForEvent(
                () -> require(editor.performAction(AccessibilityNodeInfo.ACTION_SET_SELECTION, selection), "Set selection was rejected"),
                candidate -> candidate.getEventType() == AccessibilityEvent.TYPE_VIEW_TEXT_SELECTION_CHANGED
                        && packageName.contentEquals(candidate.getPackageName())
                        && candidate.getFromIndex() == anchor && candidate.getToIndex() == focus,
                35_000);
        require(event.getItemCount() == length, "The selection event did not report the UTF-16 length");
    }

    private static Bundle selection(int anchor, int focus) {
        Bundle arguments = new Bundle();
        arguments.putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_START_INT, anchor);
        arguments.putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_END_INT, focus);
        return arguments;
    }

    private static boolean selectionIs(AccessibilityNodeInfo node, int anchor, int focus) {
        return node.getTextSelectionStart() == anchor && node.getTextSelectionEnd() == focus;
    }

    private void verifyInventory() throws TimeoutException {
        AccessibilityNodeInfo table = waitFor("Inventory table", node -> collectionIs(node, 9, 4));
        require(table.getCollectionInfo().getSelectionMode() == AccessibilityNodeInfo.CollectionInfo.SELECTION_MODE_MULTIPLE,
                "The inventory table must expose multiple selection");
        AccessibilityNodeInfo pump = waitFor("Pump", node -> itemIs(node, 0, 0));
        require(pump.performAction(AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS), "Pump rejected accessibility focus");
        waitFor("Pump", AccessibilityNodeInfo::isAccessibilityFocused);
        scroll(table, AccessibilityAction.ACTION_SCROLL_DOWN, true);
        waitFor("Filter", node -> itemIs(node, 4, 0));
        require(find("Pump", node -> itemIs(node, 0, 0)) == null, "The first cell is still exposed after scrolling out of view");
        require(!pump.performAction(AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS), "An offscreen cell accepted a stale focus action");

        scroll(waitFor("Inventory table", node -> collectionIs(node, 9, 4)), AccessibilityAction.ACTION_SCROLL_UP, true);
        waitFor("Pump", node -> itemIs(node, 0, 0) && !node.isAccessibilityFocused());
        scroll(waitFor("Inventory table", node -> collectionIs(node, 9, 4)), AccessibilityAction.ACTION_SCROLL_RIGHT, false);
        waitFor("Ready", node -> itemIs(node, 0, 3));
        waitFor("Pump", node -> itemIs(node, 0, 0));

        click(waitFor("Open log", AccessibilityNodeInfo::isClickable));
        waitFor("Inspection entries", node -> collectionIs(node, 12, 1));
        waitFor("Intake 4", AccessibilityNodeInfo::isHeading);
        click(waitFor("Insert intake", AccessibilityNodeInfo::isClickable));
        AccessibilityNodeInfo log = waitFor("Inspection entries", node -> collectionIs(node, 13, 1));
        waitFor("Bracket intake", node -> inItem(node, 1, 0));
        waitFor("Scrolls 0", node -> true);
        require(find("Rush intake", node -> true) == null, "Inserting a record moved the current scroll anchor");
        scroll(log, AccessibilityAction.ACTION_SCROLL_UP, true);
        waitFor("Rush intake", node -> inItem(node, 0, 0));
        require(automation.performGlobalAction(AccessibilityService.GLOBAL_ACTION_BACK), "Android rejected the Back action");

        click(waitFor("Open catalog", AccessibilityNodeInfo::isClickable));
        AccessibilityNodeInfo grid = waitFor("Inventory parts", node -> collectionIs(node, 7, 2));
        require("android.widget.GridView".contentEquals(grid.getClassName()), "The parts catalog must expose a GridView role");
        waitFor("Valve", node -> itemIs(node, 0, 1));
        AccessibilityNodeInfo pin = waitFor("Pin Valve", AccessibilityNodeInfo::isClickable);
        scroll(grid, AccessibilityAction.ACTION_SCROLL_DOWN, true);
        require(find("Pin Valve", node -> true) == null, "The grid still exposes an offscreen delegate");
        require(!pin.performAction(AccessibilityNodeInfo.ACTION_CLICK), "A recycled delegate accepted a stale click");

        require(automation.performGlobalAction(AccessibilityService.GLOBAL_ACTION_BACK), "Android rejected Back from the catalog");
        verifyDirectory();
    }

    private void verifyDirectory() throws TimeoutException {
        click(waitFor("Open directory", AccessibilityNodeInfo::isClickable));
        AccessibilityNodeInfo tree = waitFor("Inspection folders", node -> collectionIs(node, 2, 1));
        require(tree.getCollectionInfo().isHierarchical(), "The directory did not expose a hierarchical collection");
        require(tree.getCollectionInfo().getSelectionMode() == AccessibilityNodeInfo.CollectionInfo.SELECTION_MODE_MULTIPLE,
                "The directory did not expose multiple selection");

        AccessibilityNodeInfo workshop = waitFor("Workshop", node -> itemIs(node, 0, 0)
                && hasAction(node, AccessibilityAction.ACTION_EXPAND));
        CharSequence collapsed = workshop.getStateDescription();
        require(collapsed != null && collapsed.length() > 0, "The collapsed folder did not describe its state");
        require(find("Pump check", node -> true) == null, "A collapsed folder exposed its grandchild");
        changeExpansion(workshop, "Workshop", true);
        AccessibilityNodeInfo expanded = waitFor("Workshop", node -> hasAction(node, AccessibilityAction.ACTION_COLLAPSE));
        require(expanded.getStateDescription() != null && !collapsed.toString().contentEquals(expanded.getStateDescription()),
                "Expanding the folder did not update its state description");
        reject(expanded, AccessibilityAction.ACTION_EXPAND);
        require(!workshop.performAction(AccessibilityNodeInfo.ACTION_EXPAND), "An outdated folder node accepted Expand again");
        waitFor("Inspection folders", node -> collectionIs(node, 3, 1));
        changeExpansion(waitFor("Reports", node -> hasAction(node, AccessibilityAction.ACTION_EXPAND)), "Reports", true);
        waitFor("Inspection folders", node -> collectionIs(node, 5, 1));
        AccessibilityNodeInfo pump = waitFor("Pump check", node -> itemIs(node, 2, 0) && node.isClickable());
        click(pump);
        waitFor("Selected 21", node -> true);
        waitFor("Pump check", node -> itemIs(node, 2, 0) && node.isSelected());

        changeExpansion(waitFor("Workshop", node -> hasAction(node, AccessibilityAction.ACTION_COLLAPSE)), "Workshop", false);
        waitFor("Inspection folders", node -> collectionIs(node, 2, 1));
        waitFor("Cursor 10", node -> true);
        waitFor("Selected 21", node -> true);
        require(find("Pump check", node -> true) == null, "The collapsed folder still exposed its grandchild");
        require(!pump.performAction(AccessibilityNodeInfo.ACTION_CLICK), "The hidden child accepted a stale click");

        changeExpansion(waitFor("Workshop", node -> hasAction(node, AccessibilityAction.ACTION_EXPAND)), "Workshop", true);
        waitFor("Pump check", node -> itemIs(node, 2, 0) && node.isSelected());
        waitFor("Expanded 3", node -> true);
        waitFor("Collapsed 1", node -> true);

        AccessibilityNodeInfo archive = waitFor("Archive", node -> hasAction(node, AccessibilityAction.ACTION_EXPAND));
        click(waitFor("Find old notes", AccessibilityNodeInfo::isClickable));
        waitFor("Old notes", node -> itemIs(node, 1, 0));
        AccessibilityNodeInfo matched = waitFor("Archive", node -> itemIs(node, 0, 0));
        reject(matched, AccessibilityAction.ACTION_EXPAND);
        reject(matched, AccessibilityAction.ACTION_COLLAPSE);
        require(!archive.performAction(AccessibilityNodeInfo.ACTION_EXPAND), "A pre-filter node expanded a search result");
        waitFor("Selected 21", node -> true);
        click(waitFor("Show all", AccessibilityNodeInfo::isClickable));
        waitFor("Archive", node -> hasAction(node, AccessibilityAction.ACTION_EXPAND));
        waitFor("Expanded 3", node -> true);
    }

    private void changeExpansion(AccessibilityNodeInfo node, String label, boolean expand) throws TimeoutException {
        AccessibilityAction action = expand ? AccessibilityAction.ACTION_EXPAND : AccessibilityAction.ACTION_COLLAPSE;
        automation.executeAndWaitForEvent(
                () -> perform(node, action),
                event -> event.getEventType() == AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED
                        && packageName.contentEquals(event.getPackageName())
                        && (event.getContentChangeTypes() & AccessibilityEvent.CONTENT_CHANGE_TYPE_STATE_DESCRIPTION) != 0
                        && label.equals(String.valueOf(event.getContentDescription())),
                35_000);
    }

    private static boolean collectionIs(AccessibilityNodeInfo node, int rows, int columns) {
        AccessibilityNodeInfo.CollectionInfo info = node.getCollectionInfo();
        return info != null && info.getRowCount() == rows && info.getColumnCount() == columns;
    }

    private static boolean itemIs(AccessibilityNodeInfo node, int row, int column) {
        AccessibilityNodeInfo.CollectionItemInfo info = node.getCollectionItemInfo();
        return info != null && info.getRowIndex() == row && info.getColumnIndex() == column;
    }

    private static boolean inItem(AccessibilityNodeInfo node, int row, int column) {
        for (AccessibilityNodeInfo item = node; item != null; item = item.getParent()) {
            if (itemIs(item, row, column)) { return true; }
        }
        return false;
    }

    private void scroll(AccessibilityNodeInfo node, AccessibilityAction action, boolean vertical) throws TimeoutException {
        AccessibilityEvent event = automation.executeAndWaitForEvent(
                () -> require(node.performAction(action.getId()), "Scroll action was rejected for " + node.getText()),
                candidate -> candidate.getEventType() == AccessibilityEvent.TYPE_VIEW_SCROLLED
                        && packageName.contentEquals(candidate.getPackageName())
                        && (vertical ? candidate.getScrollDeltaY() != 0 : candidate.getScrollDeltaX() != 0),
                35_000);
        int offset = vertical ? event.getScrollY() : event.getScrollX();
        int maximum = vertical ? event.getMaxScrollY() : event.getMaxScrollX();
        require(maximum > 0 && offset >= 0 && offset <= maximum, "Scroll event did not publish valid bounds");
    }

    private void verify() throws TimeoutException {
        AccessibilityNodeInfo mute = waitFor("Mute", node -> node.isCheckable() && !node.isChecked());
        require("android.widget.Switch".contentEquals(mute.getClassName()), "Mute must expose a Switch role");
        require(mute.performAction(AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS), "Mute rejected accessibility focus");
        waitFor("Mute", AccessibilityNodeInfo::isAccessibilityFocused);

        AccessibilityNodeInfo slider = waitFor("Volume control", node -> rangeIs(node, 25));
        require("android.widget.SeekBar".contentEquals(slider.getClassName()), "Volume must expose a SeekBar role");
        Bundle progress = new Bundle();
        progress.putFloat(AccessibilityNodeInfo.ACTION_ARGUMENT_PROGRESS_VALUE, 65);
        int setProgress = AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_PROGRESS.getId();
        require(slider.performAction(setProgress, progress), "The enabled slider rejected Set progress");
        waitFor("Volume 65", node -> true);
        waitFor("Volume control", node -> rangeIs(node, 65));

        click(waitFor("Mute", node -> !node.isChecked()));
        waitFor("Mute", AccessibilityNodeInfo::isChecked);
        AccessibilityNodeInfo disabled = waitFor("Volume control", node -> !node.isEnabled());
        progress.putFloat(AccessibilityNodeInfo.ACTION_ARGUMENT_PROGRESS_VALUE, 10);
        require(!disabled.performAction(setProgress, progress), "The disabled slider accepted Set progress");
        waitFor("Volume 65", node -> true);
        click(waitFor("Mute", AccessibilityNodeInfo::isChecked));
        waitFor("Volume control", AccessibilityNodeInfo::isEnabled);

        AccessibilityNodeInfo background = waitFor("Background 0", AccessibilityNodeInfo::isClickable);
        click(waitFor("Open dialog", AccessibilityNodeInfo::isClickable));
        AccessibilityNodeInfo apply = waitFor("Apply", AccessibilityNodeInfo::isClickable);
        require(find("Background 0", node -> true) == null, "The modal tree exposes the background button");
        require(!background.performAction(AccessibilityNodeInfo.ACTION_CLICK), "A stale background node accepted a click");
        require(apply.performAction(AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS), "Apply rejected accessibility focus");
        click(waitFor("Apply", AccessibilityNodeInfo::isAccessibilityFocused));
        waitFor("Accepted 1", node -> true);
        waitFor("Background 0", AccessibilityNodeInfo::isClickable);
        require(find("Apply", node -> true) == null, "The dismissed dialog is still present");

        click(waitFor("Open profile", AccessibilityNodeInfo::isClickable));
        changeExpansion(waitFor("Advanced target", node -> hasAction(node, AccessibilityAction.ACTION_EXPAND)),
                "Advanced target", true);
        AccessibilityNodeInfo editor = waitFor("Server target", AccessibilityNodeInfo::isEditable);
        Bundle text = new Bundle();
        text.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, "inspection.local");
        require(editor.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, text), "The editor rejected Set text");
        waitFor("Draft inspection.local", node -> true);
        require(waitFor("Server target", AccessibilityNodeInfo::isEditable).performAction(AccessibilityNodeInfo.ACTION_FOCUS),
                "The editor rejected input focus");
        waitFor("Server target", AccessibilityNodeInfo::isFocused);
        changeExpansion(waitFor("Advanced target", node -> hasAction(node, AccessibilityAction.ACTION_COLLAPSE)),
                "Advanced target", false);
        waitFor("Advanced target", node -> hasAction(node, AccessibilityAction.ACTION_EXPAND));
        require(find("Server target", node -> true) == null, "The collapsed expander still exposed its editor");
        waitFor("Draft inspection.local", node -> true);
        changeExpansion(waitFor("Advanced target", node -> hasAction(node, AccessibilityAction.ACTION_EXPAND)),
                "Advanced target", true);
        waitFor("Server target", node -> "inspection.local".contentEquals(node.getText()));
        AccessibilityNodeInfo openHeader = waitFor("Advanced target", node -> hasAction(node, AccessibilityAction.ACTION_COLLAPSE));
        click(waitFor("Disable delivery", AccessibilityNodeInfo::isClickable));
        AccessibilityNodeInfo disabledHeader = waitFor("Advanced target", node -> !node.isEnabled()
                && "android.widget.Button".contentEquals(node.getClassName()));
        require(openHeader.getStateDescription() != null
                        && openHeader.getStateDescription().toString().contentEquals(disabledHeader.getStateDescription()),
                "Disabling the group lost the expander's state description");
        reject(disabledHeader, AccessibilityAction.ACTION_COLLAPSE);
        require(!openHeader.performAction(AccessibilityNodeInfo.ACTION_COLLAPSE), "A disabled expander accepted a stale collapse");
    }

    private static boolean rangeIs(AccessibilityNodeInfo node, int value) {
        AccessibilityNodeInfo.RangeInfo range = node.getRangeInfo();
        return range != null && range.getMin() == 0 && range.getMax() == 100 && range.getCurrent() == value;
    }

    private static void click(AccessibilityNodeInfo node) {
        require(node.performAction(AccessibilityNodeInfo.ACTION_CLICK), "Accessibility click was rejected for " + node.getText());
    }

    private AccessibilityNodeInfo waitFor(String label, Predicate<AccessibilityNodeInfo> state) {
        long deadline = SystemClock.uptimeMillis() + 35_000;
        do {
            AccessibilityNodeInfo node = find(label, state);
            if (node != null) { return node; }
            SystemClock.sleep(120);
        } while (SystemClock.uptimeMillis() < deadline);
        throw new AssertionError("Timed out waiting for " + label + " and its expected state. Last node: "
                + find(label, node -> true));
    }

    private AccessibilityNodeInfo find(String label, Predicate<AccessibilityNodeInfo> state) {
        AccessibilityNodeInfo root = automation.getRootInActiveWindow();
        if (root == null || !packageName.contentEquals(root.getPackageName())) { return null; }
        ArrayDeque<AccessibilityNodeInfo> pending = new ArrayDeque<>();
        pending.add(root);
        while (!pending.isEmpty()) {
            AccessibilityNodeInfo node = pending.remove();
            if ((label.equals(String.valueOf(node.getText())) || label.equals(String.valueOf(node.getContentDescription())))
                    && state.test(node)) {
                return node;
            }
            for (int slot = 0; slot < node.getChildCount(); slot++) {
                AccessibilityNodeInfo child = node.getChild(slot);
                if (child != null) { pending.add(child); }
            }
        }
        return null;
    }

    private static void require(boolean condition, String message) {
        if (!condition) { throw new AssertionError(message); }
    }
}
