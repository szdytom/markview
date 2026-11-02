package io.github.szdytom.markview;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import java.io.File;
import java.io.FileNotFoundException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;

/** External content URIs exercise the Android document-import path. */
public class FixtureProvider extends ContentProvider {
    @Override public boolean onCreate() { return true; }
    @Override public String getType(Uri uri) { return "text/markdown"; }
    private String name(Uri uri) throws FileNotFoundException {
        String name = uri.getLastPathSegment();
        if (!"root".equals(name) && !"reader.md".equals(name) && !"mermaid.md".equals(name) && !"second.md".equals(name) && !"README.md".equals(name) && !"chapter.md".equals(name) && !"images/logo.svg".equals(name) && !"images".equals(name) && !"Test.otf".equals(name)) throw new FileNotFoundException();
        return name;
    }
    @Override public Cursor query(Uri uri, String[] projection, String selection, String[] args, String sort) {
        try {
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
            String asset = "README.md".equals(name) ? "folder.md" : "chapter.md".equals(name) ? "second.md" : name;
            File file = new File(getContext().getCacheDir(), asset);
            try (InputStream input = getContext().getAssets().open(asset)) {
                Files.copy(input, file.toPath(), StandardCopyOption.REPLACE_EXISTING);
            }
            return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY);
        } catch (Exception error) { throw new FileNotFoundException(error.toString()); }
    }
    @Override public Uri insert(Uri uri, ContentValues values) { throw new UnsupportedOperationException(); }
    @Override public int delete(Uri uri, String selection, String[] args) { throw new UnsupportedOperationException(); }
    @Override public int update(Uri uri, ContentValues values, String selection, String[] args) { throw new UnsupportedOperationException(); }
}
