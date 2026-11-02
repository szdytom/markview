package io.github.szdytom.markview;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.Environment;
import android.os.ParcelFileDescriptor;
import android.os.storage.StorageManager;
import android.os.storage.StorageVolume;
import android.provider.OpenableColumns;
import android.webkit.MimeTypeMap;
import java.io.File;
import java.io.FileNotFoundException;
import java.io.IOException;
import java.util.ArrayList;

/** Read-only grants to app files and folders opened on shared storage. */
public class ReaderProvider extends ContentProvider {
    static Uri uri(Context context, File file) throws IOException {
        String path = file.getCanonicalPath();
        if (!allowed(context, path)) throw new IOException("File outside the readable directories");
        return new Uri.Builder().scheme("content").authority("io.github.szdytom.markview.files").path(path).build();
    }
    private static boolean allowed(Context context, String path) throws IOException {
        ArrayList<File> roots = new ArrayList<>();
        roots.add(context.getFilesDir());
        roots.add(Environment.getExternalStorageDirectory());
        for (StorageVolume volume : ((StorageManager)context.getSystemService(Context.STORAGE_SERVICE)).getStorageVolumes()) {
            File directory = volume.getDirectory();
            if (directory != null) roots.add(directory);
        }
        for (File root : roots) {
            String base = root.getCanonicalPath();
            if (path.equals(base) || path.startsWith(base + "/")) return true;
        }
        return false;
    }
    private File file(Uri uri) throws FileNotFoundException {
        try {
            File file = new File(uri.getPath());
            if (!allowed(getContext(), file.getCanonicalPath())) throw new FileNotFoundException("Invalid file path");
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
