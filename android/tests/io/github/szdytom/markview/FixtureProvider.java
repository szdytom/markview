package io.github.szdytom.markview;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.provider.DocumentsContract;
import java.io.File;
import java.io.FileNotFoundException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;

/** External content URIs exercise the real Android document-import path. */
public class FixtureProvider extends ContentProvider {
    private volatile boolean chapter = true;
    private volatile boolean failCopy;
    private volatile boolean image = true;
    @Override public Bundle call(String method, String arg, Bundle extras) {
        if (!"folder".equals(method)) return super.call(method, arg, extras);
        chapter = extras.getBoolean("chapter", true);
        failCopy = extras.getBoolean("failCopy");
        image = extras.getBoolean("image", true);
        return Bundle.EMPTY;
    }
    @Override public boolean onCreate() { return true; }
    @Override public String getType(Uri uri) { return "text/markdown"; }
    private String name(Uri uri) throws FileNotFoundException {
        String name = uri.getLastPathSegment();
        if (!"reader.md".equals(name) && !"second.md".equals(name) && !"README.md".equals(name) && !"chapter.md".equals(name) && !"images/logo.svg".equals(name) && !"images".equals(name) && !"Test.otf".equals(name)) throw new FileNotFoundException();
        return name;
    }
    @Override public Cursor query(Uri uri, String[] projection, String selection, String[] args, String sort) {
        try {
            if ("children".equals(uri.getLastPathSegment())) {
                MatrixCursor cursor = new MatrixCursor(new String[]{DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_MIME_TYPE});
                String id = uri.getPathSegments().get(uri.getPathSegments().size() - 2);
                if ("root".equals(id)) {
                    cursor.addRow(new Object[]{"README.md", "text/markdown"});
                    cursor.addRow(new Object[]{"images", DocumentsContract.Document.MIME_TYPE_DIR});
                    if (chapter) cursor.addRow(new Object[]{"chapter.md", "text/markdown"});
                } else if ("images".equals(id) && image) cursor.addRow(new Object[]{"images/logo.svg", "image/svg+xml"});
                return cursor;
            }
            String name = name(uri);
            MatrixCursor cursor = new MatrixCursor(new String[]{OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE});
            cursor.addRow(new Object[]{new File(name).getName(), 12000});
            return cursor;
        } catch (FileNotFoundException error) { return null; }
    }
    @Override public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        try {
            if (!"r".equals(mode)) throw new FileNotFoundException();
            String name = name(uri);
            if (failCopy && "images/logo.svg".equals(name)) throw new FileNotFoundException("Interrupted folder import");
            String asset = "README.md".equals(name) ? "folder.md" : "chapter.md".equals(name) ? "second.md" : "images/logo.svg".equals(name) ? "logo.svg" : name;
            File file = new File(getContext().getCacheDir(), asset);
            try (InputStream input = getContext().getAssets().open(asset)) {
                Files.copy(input, file.toPath(), StandardCopyOption.REPLACE_EXISTING);
            }
            if (failCopy && "README.md".equals(name)) Files.write(file.toPath(), "Incomplete import".getBytes(java.nio.charset.StandardCharsets.UTF_8));
            return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY);
        } catch (Exception error) { throw new FileNotFoundException(error.toString()); }
    }
    @Override public Uri insert(Uri uri, ContentValues values) { throw new UnsupportedOperationException(); }
    @Override public int delete(Uri uri, String selection, String[] args) { throw new UnsupportedOperationException(); }
    @Override public int update(Uri uri, ContentValues values, String selection, String[] args) { throw new UnsupportedOperationException(); }
}
