package io.github.szdytom.markview;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import android.webkit.MimeTypeMap;
import java.io.File;
import java.io.FileNotFoundException;
import java.io.IOException;

/** Read-only grants to exports and settings inside the private app directory. */
public class ReaderProvider extends ContentProvider {
    static Uri uri(Context context, File file) throws IOException {
        String base = context.getFilesDir().getCanonicalPath() + "/";
        String path = file.getCanonicalPath();
        if (!path.startsWith(base)) throw new IOException("File outside the app directory");
        return new Uri.Builder().scheme("content").authority("io.github.szdytom.markview.files").path(path.substring(base.length())).build();
    }
    private File file(Uri uri) throws FileNotFoundException {
        try {
            File file = new File(getContext().getFilesDir(), uri.getPath());
            String base = getContext().getFilesDir().getCanonicalPath() + "/";
            if (!file.getCanonicalPath().startsWith(base)) throw new FileNotFoundException("Invalid file path");
            return file;
        } catch (IOException error) { throw new FileNotFoundException(error.getMessage()); }
    }
    @Override public boolean onCreate() { return true; }
    @Override public String getType(Uri uri) {
        String path = uri.getPath();
        return MimeTypeMap.getSingleton().getMimeTypeFromExtension(path.substring(path.lastIndexOf('.') + 1));
    }
    @Override public Cursor query(Uri uri, String[] projection, String selection, String[] args, String sort) {
        try {
            File file = file(uri);
            String[] columns = projection == null ? new String[]{OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE} : projection;
            MatrixCursor cursor = new MatrixCursor(columns);
            Object[] values = new Object[columns.length];
            for (int i = 0; i < columns.length; i++) values[i] = OpenableColumns.DISPLAY_NAME.equals(columns[i]) ? file.getName() : file.length();
            cursor.addRow(values);
            return cursor;
        } catch (FileNotFoundException error) { return null; }
    }
    @Override public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        if (!"r".equals(mode)) throw new FileNotFoundException("Read-only provider");
        return ParcelFileDescriptor.open(file(uri), ParcelFileDescriptor.MODE_READ_ONLY);
    }
    @Override public Uri insert(Uri uri, ContentValues values) { throw new UnsupportedOperationException(); }
    @Override public int delete(Uri uri, String selection, String[] args) { throw new UnsupportedOperationException(); }
    @Override public int update(Uri uri, ContentValues values, String selection, String[] args) { throw new UnsupportedOperationException(); }
}
