package dev.xrack.platform;

import android.app.AlertDialog;
import android.app.NativeActivity;
import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.ClipData;
import android.content.Context;
import android.content.Intent;
import android.os.Build;
import android.database.Cursor;
import android.net.Uri;
import android.provider.OpenableColumns;
import android.provider.DocumentsContract;
import android.webkit.MimeTypeMap;
import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.ByteArrayOutputStream;
import java.nio.file.Files;
import java.lang.ref.WeakReference;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.Locale;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;

public class XRackActivity extends NativeActivity {
    private static final int PICK_FILES = 24171;
    private static final ConcurrentHashMap<String, WeakReference<XRackActivity>> SHARES =
            new ConcurrentHashMap<>();
    private long pending;
    private int pendingKind;
    private String exportSource;
    private String[] allowedExtensions;
    private String shareAction;
    private PendingIntent shareCallback;
    private boolean shareVisible;
    private XRackTextInput textInput;

    native void editText(long token, long epoch, long serial, int kind,
            String value, String preedit, int start, int end, int anchorStart, int anchorEnd);
    native void imeInsets(long token, int top);

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

    private void showDialog(long token, int kind, String title, String text, String[] extensions) {
        runOnUiThread(() -> {
            if (isFinishing() || isDestroyed() || pending != 0) {
                finishDialog(token, -1, "The Android activity cannot open another dialog.");
                return;
            }
            pending = token;
            pendingKind = kind;
            try {
                if (kind < 2) {
                    openFiles(kind == 1, title, extensions);
                } else if (kind == 6) {
                    startActivityForResult(new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
                            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION), PICK_FILES);
                } else if (kind == 7) {
                    exportSource = text;
                    startActivityForResult(new Intent(Intent.ACTION_CREATE_DOCUMENT)
                            .addCategory(Intent.CATEGORY_OPENABLE)
                            .setType("application/octet-stream")
                            .putExtra(Intent.EXTRA_TITLE, title)
                            .addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION), PICK_FILES);
                } else if (kind == 8) {
                    shareText(token, text);
                } else {
                    showMessage(token, kind - 2, title, text);
                }
            } catch (RuntimeException error) {
                complete(token, -1, error.toString());
            }
        });
    }

    private void shareText(long token, String text) {
        String action = getPackageName() + ".XRACK_SHARE." + token;
        shareAction = action;
        SHARES.put(action, new WeakReference<>(this));
        int flags = PendingIntent.FLAG_ONE_SHOT | PendingIntent.FLAG_CANCEL_CURRENT;
        if (Build.VERSION.SDK_INT >= 31) {
            flags |= PendingIntent.FLAG_MUTABLE;
        }
        shareCallback = PendingIntent.getBroadcast(this, 0,
                new Intent(this, ShareResultReceiver.class).setAction(action), flags);
        Intent send = new Intent(Intent.ACTION_SEND)
                .setType("text/plain")
                .putExtra(Intent.EXTRA_TEXT, text);
        shareVisible = false;
        startActivity(Intent.createChooser(send, null, shareCallback.getIntentSender()));
    }

    public static final class ShareResultReceiver extends BroadcastReceiver {
        @Override
        public void onReceive(Context context, Intent result) {
            String action = result.getAction();
            if (action == null || !result.hasExtra(Intent.EXTRA_CHOSEN_COMPONENT)) {
                return;
            }
            WeakReference<XRackActivity> reference = SHARES.get(action);
            XRackActivity activity = reference == null ? null : reference.get();
            if (activity != null && action.equals(activity.shareAction) && activity.pendingKind == 8) {
                activity.complete(activity.pending, 2, "");
            }
        }
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (pending != 0 && pendingKind == 8) {
            if (hasFocus && shareVisible) {
                complete(pending, 0, "");
            } else if (!hasFocus) {
                shareVisible = true;
            }
        }
    }

    private void openFiles(boolean multiple, String title, String[] extensions) {
        allowedExtensions = extensions;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT)
                .addCategory(Intent.CATEGORY_OPENABLE)
                .setType("*/*")
                .putExtra(Intent.EXTRA_ALLOW_MULTIPLE, multiple)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        if (!title.isEmpty()) {
            intent.putExtra(Intent.EXTRA_TITLE, title);
        }
        Set<String> types = new LinkedHashSet<>();
        for (String extension : extensions) {
            String type = MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension);
            if (type == null) {
                types.clear();
                break;
            }
            types.add(type);
        }
        if (types.size() == 1) {
            intent.setType(types.iterator().next());
        } else if (!types.isEmpty()) {
            intent.putExtra(Intent.EXTRA_MIME_TYPES, types.toArray(new String[0]));
        }
        startActivityForResult(intent, PICK_FILES);
    }

    private void showMessage(long token, int buttons, String title, String text) {
        boolean question = buttons >= 2;
        AlertDialog.Builder builder = new AlertDialog.Builder(this)
                .setTitle(title)
                .setMessage(text)
                .setPositiveButton(question ? R.string.xrack_dialog_yes : android.R.string.ok,
                        (dialog, which) -> complete(token, question ? 3 : 2, ""));
        if (question) {
            builder.setNegativeButton(R.string.xrack_dialog_no, (dialog, which) -> complete(token, 4, ""));
        }
        if (buttons == 1 || buttons == 3) {
            builder.setNeutralButton(android.R.string.cancel, (dialog, which) -> complete(token, 5, ""));
        }
        builder.setOnCancelListener(dialog -> complete(token, 5, "")).show();
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != PICK_FILES || pending == 0) {
            return;
        }
        long token = pending;
        if (result != RESULT_OK || data == null) {
            complete(token, 0, "");
            return;
        }
        if (pendingKind == 6) {
            Uri directory = data.getData();
            complete(token, directory == null ? 0 : 1, directory == null ? "" : directory.toString());
            return;
        }
        if (pendingKind == 7) {
            Uri destination = data.getData();
            if (destination == null) {
                complete(token, 0, "");
                return;
            }
            String source = exportSource;
            new Thread(() -> exportDocument(token, source, destination), "xrack-file-export").start();
            return;
        }
        Set<Uri> documents = new LinkedHashSet<>();
        ClipData selection = data.getClipData();
        if (selection != null) {
            for (int i = 0; i < selection.getItemCount(); i++) {
                documents.add(selection.getItemAt(i).getUri());
            }
        } else if (data.getData() != null) {
            documents.add(data.getData());
        }
        if (documents.isEmpty()) {
            complete(token, 0, "");
            return;
        }
        String[] extensions = allowedExtensions;
        new Thread(() -> importDocuments(token, documents, extensions), "xrack-file-picker").start();
    }

    private void exportDocument(long token, String source, Uri destination) {
        try (OutputStream output = getContentResolver().openOutputStream(destination, "wt")) {
            if (output == null) {
                throw new IOException("The selected document cannot be written.");
            }
            Files.copy(new File(source).toPath(), output);
        } catch (IOException | RuntimeException error) {
            runOnUiThread(() -> complete(token, -1, error.toString()));
            return;
        }
        runOnUiThread(() -> complete(token, 2, ""));
    }

    private String[] listDocuments(String location) throws IOException {
        Uri directory = Uri.parse(location);
        String id = DocumentsContract.isDocumentUri(this, directory)
                ? DocumentsContract.getDocumentId(directory) : DocumentsContract.getTreeDocumentId(directory);
        Uri children = DocumentsContract.buildChildDocumentsUriUsingTree(directory, id);
        ArrayList<String> entries = new ArrayList<>();
        String[] projection = {DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                DocumentsContract.Document.COLUMN_DISPLAY_NAME, DocumentsContract.Document.COLUMN_MIME_TYPE};
        try (Cursor cursor = getContentResolver().query(children, projection, null, null, null)) {
            if (cursor == null) {
                throw new IOException("The directory cannot be read.");
            }
            while (cursor.moveToNext()) {
                entries.add(cursor.getString(1));
                entries.add(DocumentsContract.buildDocumentUriUsingTree(directory, cursor.getString(0)).toString());
                entries.add(cursor.getString(2));
            }
        }
        return entries.toArray(new String[0]);
    }

    private byte[] readDocument(String location, int maxBytes) throws IOException {
        try (InputStream input = getContentResolver().openInputStream(Uri.parse(location))) {
            if (input == null) {
                throw new IOException("The document cannot be opened.");
            }
            ByteArrayOutputStream output = new ByteArrayOutputStream();
            byte[] buffer = new byte[8192];
            int length;
            while ((length = input.read(buffer)) != -1) {
                if (length > maxBytes - output.size()) {
                    throw new IOException("The document exceeds the requested byte limit.");
                }
                output.write(buffer, 0, length);
            }
            return output.toByteArray();
        }
    }

    private void importDocuments(long token, Set<Uri> documents, String[] extensions) {
        ArrayList<File> staging = new ArrayList<>();
        ArrayList<String> paths = new ArrayList<>();
        try {
            // A provider's URI cannot be opened as a filesystem path. Give the
            // application private cache copies whose names retain the extensions.
            for (Uri uri : documents) {
                String name = documentName(uri, extensions);
                File directory = Files.createTempDirectory(getCacheDir().toPath(), "xrack-file-").toFile();
                staging.add(directory);
                File file = new File(directory, name);
                staging.add(file);
                try (InputStream input = getContentResolver().openInputStream(uri)) {
                    if (input == null) {
                        throw new IOException("The selected document could not be opened.");
                    }
                    Files.copy(input, file.toPath());
                }
                paths.add(file.getAbsolutePath());
            }
            String payload = String.join("\0", paths);
            runOnUiThread(() -> {
                if (pending == token) {
                    complete(token, 1, payload);
                } else {
                    removeStaging(staging);
                }
            });
        } catch (IOException | RuntimeException error) {
            removeStaging(staging);
            runOnUiThread(() -> complete(token, -1, error.toString()));
        }
    }

    private String documentName(Uri uri, String[] extensions) throws IOException {
        String name = null;
        try (Cursor cursor = getContentResolver().query(
                uri, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                name = cursor.getString(0);
            }
        }
        if (name == null || name.isEmpty() || name.equals(".") || name.equals("..")) {
            name = "document";
        }
        name = name.replace('/', '_').replace('\\', '_').replace('\0', '_');
        if (extensions.length > 0) {
            String lower = name.toLowerCase(Locale.ROOT);
            for (String extension : extensions) {
                if (lower.endsWith("." + extension)) {
                    return name;
                }
            }
            throw new IOException("The selected document does not match the requested file types.");
        }
        return name;
    }

    private static void removeStaging(ArrayList<File> staging) {
        for (int i = staging.size() - 1; i >= 0; i--) {
            staging.get(i).delete();
        }
    }

    private void complete(long token, int result, String payload) {
        if (pending == token && token != 0) {
            pending = 0;
            allowedExtensions = null;
            exportSource = null;
            if (shareAction != null) {
                SHARES.remove(shareAction);
                shareAction = null;
            }
            if (shareCallback != null) {
                shareCallback.cancel();
                shareCallback = null;
            }
            shareVisible = false;
            finishDialog(token, result, payload);
        }
    }

    @Override
    protected void onDestroy() {
        complete(pending, -1, "The Android activity was closed.");
        super.onDestroy();
    }
}
