package dev.xrack.platform;

import android.graphics.Rect;
import android.os.Build;
import android.os.Bundle;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewParent;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityManager;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeProvider;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

final class XRackAccessibility extends View {
    // Node and action flags mirror android_accessibility.rs.
    private static final int CLICK = 1, FOCUS = 2, SET_TEXT = 4, SET_PROGRESS = 8;
    private static final int UP = 16, DOWN = 32, LEFT = 64, RIGHT = 128, INCREMENT = 256, DECREMENT = 512;
    private static final int SET_SELECTION = 1024;
    private static final int COPY = 2048, CUT = 4096, PASTE = 8192;
    private static final int EXPAND = 16384, COLLAPSE = 32768;
    private static final int TEXT_GRANULARITIES = AccessibilityNodeInfo.MOVEMENT_GRANULARITY_CHARACTER
            | AccessibilityNodeInfo.MOVEMENT_GRANULARITY_WORD;
    private static final int DISABLED = 1, CHECKABLE = 2, CHECKED = 4, SELECTED = 8;
    private static final int PASSWORD = 16, MULTILINE = 32, MIXED = 64, HEADING = 128;
    private static final int HIERARCHICAL = 256;
    private static final int EXPANDABLE = 512, EXPANDED = 1024;
    private static final int HOST = AccessibilityNodeProvider.HOST_VIEW_ID;

    private final XRackActivity activity;
    final long token;
    private final AccessibilityManager manager;
    private final AccessibilityManager.AccessibilityStateChangeListener listener = this::setActive;
    private final Provider provider = new Provider();
    private Snapshot snapshot = new Snapshot();
    private boolean active;
    private int accessibilityFocus = HOST;
    private int hovered = HOST;

    XRackAccessibility(XRackActivity activity, long token) {
        super(activity);
        this.activity = activity;
        this.token = token;
        manager = activity.getSystemService(AccessibilityManager.class);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
        setFocusable(false);
        setClickable(false);
    }

    @Override protected void onAttachedToWindow() {
        super.onAttachedToWindow();
        manager.addAccessibilityStateChangeListener(listener);
        setActive(manager.isEnabled());
    }

    @Override protected void onDetachedFromWindow() {
        manager.removeAccessibilityStateChangeListener(listener);
        setActive(false);
        super.onDetachedFromWindow();
    }

    private void setActive(boolean enabled) {
        if (active == enabled) { return; }
        active = enabled;
        if (!active) {
            snapshot = new Snapshot();
            accessibilityFocus = HOST;
            hovered = HOST;
        }
        activity.accessibilityEnabled(token, active);
    }

    void publish(int inputFocus, int[] data, float[] ranges, String[] strings) {
        if (!active) { return; }
        Snapshot next = new Snapshot(inputFocus, data, ranges, strings);
        if (accessibilityFocus != HOST && !next.rows.containsKey(accessibilityFocus)) {
            send(accessibilityFocus, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED);
            accessibilityFocus = HOST;
        }
        if (hovered != HOST && !next.rows.containsKey(hovered)) {
            send(hovered, AccessibilityEvent.TYPE_VIEW_HOVER_EXIT);
            hovered = HOST;
        }
        Snapshot previous = snapshot;
        snapshot = next;
        send(HOST, AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED);
        for (Map.Entry<Integer, Row> entry : next.rows.entrySet()) {
            Row before = previous.rows.get(entry.getKey());
            Row after = entry.getValue();
            if (before != null && (before.scrollX != after.scrollX || before.scrollY != after.scrollY)) {
                sendScroll(entry.getKey(), before, after);
            }
            if (before != null && after.selectionStart >= 0 && after.selectionEnd >= 0
                    && (before.selectionStart != after.selectionStart || before.selectionEnd != after.selectionEnd)) {
                sendSelection(entry.getKey(), after);
            }
            if (Build.VERSION.SDK_INT >= 30 && before != null
                    && ((before.flags ^ after.flags) & (EXPANDABLE | EXPANDED)) != 0) {
                send(entry.getKey(), AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED,
                        AccessibilityEvent.CONTENT_CHANGE_TYPE_STATE_DESCRIPTION);
            }
        }
    }

    @Override public AccessibilityNodeProvider getAccessibilityNodeProvider() { return provider; }

    private void sendScroll(int id, Row before, Row after) {
        if (getParent() == null) { return; }
        AccessibilityEvent event = AccessibilityEvent.obtain(AccessibilityEvent.TYPE_VIEW_SCROLLED);
        event.setPackageName(getContext().getPackageName());
        event.setClassName(after.className);
        event.setSource(this, id);
        event.setScrollable(true);
        event.setScrollX(after.scrollX);
        event.setScrollY(after.scrollY);
        event.setMaxScrollX(after.maxScrollX);
        event.setMaxScrollY(after.maxScrollY);
        if (Build.VERSION.SDK_INT >= 28) {
            event.setScrollDeltaX(after.scrollX - before.scrollX);
            event.setScrollDeltaY(after.scrollY - before.scrollY);
        }
        dispatchAccessibilityEvent(event);
    }

    private void sendSelection(int id, Row row) {
        if (getParent() == null) { return; }
        AccessibilityEvent event = AccessibilityEvent.obtain(AccessibilityEvent.TYPE_VIEW_TEXT_SELECTION_CHANGED);
        event.setPackageName(getContext().getPackageName());
        event.setClassName(row.className);
        event.setSource(this, id);
        event.setEnabled((row.flags & DISABLED) == 0);
        event.setFromIndex(row.selectionStart);
        event.setToIndex(row.selectionEnd);
        String text = row.value == null ? "" : row.value;
        event.setItemCount(text.length());
        event.getText().add(text);
        dispatchAccessibilityEvent(event);
    }

    void publishTraversal(int id, int start, int end, int granularity, boolean forward) {
        Row row = snapshot.rows.get(id);
        if (!active || getParent() == null || row == null || row.value == null) { return; }
        AccessibilityEvent event = AccessibilityEvent.obtain(AccessibilityEvent.TYPE_VIEW_TEXT_TRAVERSED_AT_MOVEMENT_GRANULARITY);
        event.setPackageName(getContext().getPackageName());
        event.setClassName(row.className);
        event.setSource(this, id);
        event.setEnabled((row.flags & DISABLED) == 0);
        event.setFromIndex(start);
        event.setToIndex(end);
        event.setItemCount(row.value.length());
        event.setMovementGranularity(granularity);
        event.setAction(forward ? AccessibilityNodeInfo.ACTION_NEXT_AT_MOVEMENT_GRANULARITY
                : AccessibilityNodeInfo.ACTION_PREVIOUS_AT_MOVEMENT_GRANULARITY);
        event.getText().add(row.value);
        dispatchAccessibilityEvent(event);
    }

    private static boolean isTextBoundary(String value, int offset) {
        return offset >= 0 && offset <= value.length()
                && (offset == 0 || offset == value.length()
                    || !Character.isSurrogatePair(value.charAt(offset - 1), value.charAt(offset)));
    }

    @Override public boolean dispatchHoverEvent(MotionEvent event) {
        if (!active || !manager.isTouchExplorationEnabled()) { return super.dispatchHoverEvent(event); }
        int next = HOST;
        if (event.getActionMasked() != MotionEvent.ACTION_HOVER_EXIT) {
            int[] origin = new int[2];
            getRootView().getLocationOnScreen(origin);
            next = snapshot.hit(HOST, (int) event.getRawX() - origin[0], (int) event.getRawY() - origin[1]);
        }
        if (hovered != next) {
            if (hovered != HOST) { send(hovered, AccessibilityEvent.TYPE_VIEW_HOVER_EXIT); }
            if (next != HOST) { send(next, AccessibilityEvent.TYPE_VIEW_HOVER_ENTER); }
            hovered = next;
        }
        return true;
    }

    private void send(int id, int type) {
        send(id, type, type == AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED
                ? AccessibilityEvent.CONTENT_CHANGE_TYPE_SUBTREE : 0);
    }

    private void send(int id, int type, int contentChange) {
        if (getParent() == null) { return; }
        AccessibilityEvent event = AccessibilityEvent.obtain(type);
        Row row = snapshot.rows.get(id);
        event.setPackageName(getContext().getPackageName());
        event.setSource(this, id);
        if (type == AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED) {
            event.setContentChangeTypes(contentChange);
        }
        if (row != null) {
            event.setClassName(row.className);
            event.setEnabled((row.flags & DISABLED) == 0);
            event.setContentDescription(row.label);
        }
        dispatchAccessibilityEvent(event);
    }

    private void dispatchAccessibilityEvent(AccessibilityEvent event) {
        ViewParent parent = getParent();
        if (parent == null || !manager.isEnabled()) { return; }
        try {
            parent.requestSendAccessibilityEvent(this, event);
        } catch (IllegalStateException error) {
            // The system can disable accessibility before the queued listener runs.
            if (manager.isEnabled()) { throw error; }
        }
    }

    private final class Provider extends AccessibilityNodeProvider {
        @Override public AccessibilityNodeInfo createAccessibilityNodeInfo(int id) {
            if (id == HOST) {
                AccessibilityNodeInfo info = AccessibilityNodeInfo.obtain(XRackAccessibility.this);
                onInitializeAccessibilityNodeInfo(info);
                addChildren(info, HOST);
                return info;
            }
            Row row = snapshot.rows.get(id);
            if (row == null) { return null; }
            AccessibilityNodeInfo info = AccessibilityNodeInfo.obtain();
            info.setSource(XRackAccessibility.this, id);
            if (row.parent == HOST) { info.setParent(XRackAccessibility.this); }
            else { info.setParent(XRackAccessibility.this, row.parent); }
            info.setPackageName(getContext().getPackageName());
            info.setClassName(row.className);
            info.setEnabled((row.flags & DISABLED) == 0);
            info.setCheckable((row.flags & CHECKABLE) != 0);
            info.setChecked((row.flags & CHECKED) != 0);
            info.setSelected((row.flags & SELECTED) != 0);
            info.setPassword((row.flags & PASSWORD) != 0);
            info.setMultiLine((row.flags & MULTILINE) != 0);
            info.setText(row.value == null ? row.label : row.value);
            info.setContentDescription(row.value == null ? row.description : row.label);
            if (Build.VERSION.SDK_INT >= 28) { info.setHeading((row.flags & HEADING) != 0); }
            if (Build.VERSION.SDK_INT >= 30 && (row.flags & MIXED) != 0) { info.setStateDescription(getContext().getString(R.string.xrack_mixed)); }
            if (Build.VERSION.SDK_INT >= 30 && (row.flags & EXPANDABLE) != 0) {
                info.setStateDescription(getContext().getString((row.flags & EXPANDED) != 0
                        ? R.string.xrack_expanded : R.string.xrack_collapsed));
            }
            Rect relative = new Rect(row.bounds);
            Row parent = snapshot.rows.get(row.parent);
            if (parent != null) { relative.offset(-parent.bounds.left, -parent.bounds.top); }
            info.setBoundsInParent(relative);
            int[] origin = new int[2];
            getRootView().getLocationOnScreen(origin);
            Rect screen = new Rect(row.bounds);
            screen.offset(origin[0], origin[1]);
            info.setBoundsInScreen(screen);
            info.setVisibleToUser(isShown() && !screen.isEmpty());
            info.setFocusable((row.actions & FOCUS) != 0);
            info.setFocused(snapshot.focus == id);
            info.setAccessibilityFocused(accessibilityFocus == id);
            info.setClickable((row.actions & CLICK) != 0);
            info.setEditable((row.actions & SET_TEXT) != 0);
            if (row.selectionStart >= 0 && row.selectionEnd >= 0) {
                info.setTextSelection(row.selectionStart, row.selectionEnd);
            }
            info.setScrollable((row.actions & (UP | DOWN | LEFT | RIGHT)) != 0);
            if (row.rowCount >= 0 && row.columnCount >= 0) {
                info.setCollectionInfo(AccessibilityNodeInfo.CollectionInfo.obtain(
                        row.rowCount, row.columnCount, (row.flags & HIERARCHICAL) != 0, row.selection));
            }
            if (row.rowIndex >= 0 && row.columnIndex >= 0) {
                info.setCollectionItemInfo(AccessibilityNodeInfo.CollectionItemInfo.obtain(
                        row.rowIndex, 1, row.columnIndex, 1, (row.flags & HEADING) != 0,
                        (row.flags & SELECTED) != 0));
            }
            if (Float.isFinite(row.valueNumber) && row.minimum < row.maximum) {
                info.setRangeInfo(AccessibilityNodeInfo.RangeInfo.obtain(
                        AccessibilityNodeInfo.RangeInfo.RANGE_TYPE_FLOAT, row.minimum, row.maximum, row.valueNumber));
            }
            info.addAction(accessibilityFocus == id ? AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS : AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS);
            if ((row.actions & FOCUS) != 0) { info.addAction(AccessibilityNodeInfo.ACTION_FOCUS); }
            if ((row.actions & CLICK) != 0) { info.addAction(AccessibilityNodeInfo.ACTION_CLICK); }
            if ((row.actions & SET_TEXT) != 0) { info.addAction(AccessibilityNodeInfo.ACTION_SET_TEXT); }
            if ((row.actions & SET_SELECTION) != 0) { info.addAction(AccessibilityNodeInfo.ACTION_SET_SELECTION); }
            if (row.canTraverseText()) {
                info.setMovementGranularities(TEXT_GRANULARITIES);
                info.addAction(AccessibilityNodeInfo.ACTION_NEXT_AT_MOVEMENT_GRANULARITY);
                info.addAction(AccessibilityNodeInfo.ACTION_PREVIOUS_AT_MOVEMENT_GRANULARITY);
            }
            if ((row.actions & COPY) != 0) { info.addAction(AccessibilityNodeInfo.ACTION_COPY); }
            if ((row.actions & CUT) != 0) { info.addAction(AccessibilityNodeInfo.ACTION_CUT); }
            if ((row.actions & PASTE) != 0) { info.addAction(AccessibilityNodeInfo.ACTION_PASTE); }
            if ((row.actions & EXPAND) != 0) { info.addAction(AccessibilityNodeInfo.ACTION_EXPAND); }
            if ((row.actions & COLLAPSE) != 0) { info.addAction(AccessibilityNodeInfo.ACTION_COLLAPSE); }
            if (Build.VERSION.SDK_INT >= 24 && (row.actions & SET_PROGRESS) != 0) {
                info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_PROGRESS);
            }
            if ((row.actions & (DOWN | RIGHT | INCREMENT)) != 0) { info.addAction(AccessibilityNodeInfo.ACTION_SCROLL_FORWARD); }
            if ((row.actions & (UP | LEFT | DECREMENT)) != 0) { info.addAction(AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD); }
            if ((row.actions & UP) != 0) { info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_UP); }
            if ((row.actions & DOWN) != 0) { info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_DOWN); }
            if ((row.actions & LEFT) != 0) { info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_LEFT); }
            if ((row.actions & RIGHT) != 0) { info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_RIGHT); }
            addChildren(info, id);
            return info;
        }

        private void addChildren(AccessibilityNodeInfo info, int id) {
            for (int child : snapshot.children(id)) { info.addChild(XRackAccessibility.this, child); }
        }

        @Override public AccessibilityNodeInfo findFocus(int focusType) {
            int id = focusType == AccessibilityNodeInfo.FOCUS_ACCESSIBILITY ? accessibilityFocus : snapshot.focus;
            return id == HOST ? null : createAccessibilityNodeInfo(id);
        }

        @Override public boolean performAction(int id, int action, Bundle arguments) {
            if (id == HOST) { return XRackAccessibility.this.performAccessibilityAction(action, arguments); }
            Row row = snapshot.rows.get(id);
            if (row == null) { return false; }
            if (action == AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS && accessibilityFocus != id) {
                if (accessibilityFocus != HOST) { send(accessibilityFocus, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED); }
                accessibilityFocus = id;
                send(id, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUSED);
                return true;
            }
            if (action == AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS && accessibilityFocus == id) {
                accessibilityFocus = HOST;
                send(id, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED);
                return true;
            }
            if (action == AccessibilityNodeInfo.ACTION_SET_SELECTION) {
                if ((row.actions & SET_SELECTION) == 0) { return false; }
                String value = row.value == null ? "" : row.value;
                int anchor = arguments == null ? -1 : arguments.getInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_START_INT, -1);
                int focus = arguments == null ? -1 : arguments.getInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_END_INT, -1);
                if (anchor == -1 && focus == -1) {
                    focus = row.selectionEnd >= 0 ? row.selectionEnd : value.length();
                    anchor = focus;
                }
                if (!isTextBoundary(value, anchor) || !isTextBoundary(value, focus)) { return false; }
                return activity.accessibilitySetSelection(token, id, value, anchor, focus);
            }
            if (action == AccessibilityNodeInfo.ACTION_NEXT_AT_MOVEMENT_GRANULARITY
                    || action == AccessibilityNodeInfo.ACTION_PREVIOUS_AT_MOVEMENT_GRANULARITY) {
                if (!row.canTraverseText() || arguments == null) { return false; }
                int granularity = arguments.getInt(AccessibilityNodeInfo.ACTION_ARGUMENT_MOVEMENT_GRANULARITY_INT);
                boolean forward = action == AccessibilityNodeInfo.ACTION_NEXT_AT_MOVEMENT_GRANULARITY;
                boolean extend = arguments.getBoolean(AccessibilityNodeInfo.ACTION_ARGUMENT_EXTEND_SELECTION_BOOLEAN);
                return activity.accessibilityTraverseText(token, id, row.value,
                        row.selectionStart, row.selectionEnd, granularity, forward, extend);
            }
            int code = switch (action) {
                case AccessibilityNodeInfo.ACTION_CLICK -> CLICK;
                case AccessibilityNodeInfo.ACTION_FOCUS -> FOCUS;
                case AccessibilityNodeInfo.ACTION_SET_TEXT -> SET_TEXT;
                case AccessibilityNodeInfo.ACTION_COPY -> COPY;
                case AccessibilityNodeInfo.ACTION_CUT -> CUT;
                case AccessibilityNodeInfo.ACTION_PASTE -> PASTE;
                case AccessibilityNodeInfo.ACTION_EXPAND -> EXPAND;
                case AccessibilityNodeInfo.ACTION_COLLAPSE -> COLLAPSE;
                case AccessibilityNodeInfo.ACTION_SCROLL_FORWARD -> Integer.lowestOneBit(row.actions & (DOWN | RIGHT | INCREMENT));
                case AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD -> Integer.lowestOneBit(row.actions & (UP | LEFT | DECREMENT));
                default -> 0;
            };
            if (Build.VERSION.SDK_INT >= 24 && action == AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_PROGRESS.getId()) {
                code = SET_PROGRESS;
            }
            if (action == AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_UP.getId()) { code = UP; }
            if (action == AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_DOWN.getId()) { code = DOWN; }
            if (action == AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_LEFT.getId()) { code = LEFT; }
            if (action == AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_RIGHT.getId()) { code = RIGHT; }
            if (code == 0 || (row.actions & code) == 0) { return false; }
            String text = null;
            float progress = Float.NaN;
            if (code == SET_TEXT && arguments != null) {
                CharSequence input = arguments.getCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE);
                if (input != null) { text = input.toString(); }
            }
            if (code == SET_TEXT && text == null) { return false; }
            if (code == SET_PROGRESS && arguments != null) {
                progress = arguments.getFloat(AccessibilityNodeInfo.ACTION_ARGUMENT_PROGRESS_VALUE, Float.NaN);
            }
            boolean delivered = activity.accessibilityAction(token, id, code, text, progress);
            if (delivered && code == CLICK) { send(id, AccessibilityEvent.TYPE_VIEW_CLICKED); }
            return delivered;
        }
    }

    private static final class Row {
        static final int STRIDE = 15;
        static final int RANGE_STRIDE = 7;
        final int parent, flags, actions;
        final int rowCount, columnCount, rowIndex, columnIndex, selection;
        final int selectionStart, selectionEnd;
        final int scrollX, scrollY, maxScrollX, maxScrollY;
        final Rect bounds;
        final String className, label, value, description;
        final float minimum, maximum, valueNumber;

        boolean canTraverseText() {
            return (actions & SET_SELECTION) != 0 && value != null && !value.isEmpty();
        }

        Row(int index, int[] data, float[] ranges, String[] strings) {
            int at = index * STRIDE;
            parent = data[at + 1];
            flags = data[at + 2];
            actions = data[at + 3];
            bounds = new Rect(data[at + 4], data[at + 5], data[at + 6], data[at + 7]);
            rowCount = data[at + 8];
            columnCount = data[at + 9];
            rowIndex = data[at + 10];
            columnIndex = data[at + 11];
            selection = data[at + 12];
            selectionStart = data[at + 13];
            selectionEnd = data[at + 14];
            className = strings[index * 4];
            label = strings[index * 4 + 1];
            value = strings[index * 4 + 2];
            description = strings[index * 4 + 3];
            int range = index * RANGE_STRIDE;
            minimum = ranges[range];
            maximum = ranges[range + 1];
            valueNumber = ranges[range + 2];
            scrollX = Math.round(ranges[range + 3]);
            scrollY = Math.round(ranges[range + 4]);
            maxScrollX = Math.round(ranges[range + 5]);
            maxScrollY = Math.round(ranges[range + 6]);
        }
    }

    private static final class Snapshot {
        final int focus;
        final Map<Integer, Row> rows = new HashMap<>();
        final Map<Integer, List<Integer>> children = new HashMap<>();

        Snapshot() { focus = HOST; }

        Snapshot(int focus, int[] data, float[] ranges, String[] strings) {
            this.focus = focus;
            for (int index = 0; index < data.length / Row.STRIDE; index++) {
                int id = data[index * Row.STRIDE];
                Row row = new Row(index, data, ranges, strings);
                rows.put(id, row);
                children.computeIfAbsent(row.parent, unused -> new ArrayList<>()).add(id);
            }
        }

        List<Integer> children(int id) { return children.getOrDefault(id, java.util.Collections.emptyList()); }

        int hit(int id, int x, int y) {
            List<Integer> list = children(id);
            for (int index = list.size() - 1; index >= 0; index--) {
                int child = list.get(index);
                Row row = rows.get(child);
                if (row.bounds.contains(x, y)) {
                    int hit = hit(child, x, y);
                    if (hit != HOST) { return hit; }
                    if (row.label != null || row.value != null || row.actions != 0) { return child; }
                }
            }
            return HOST;
        }
    }
}
