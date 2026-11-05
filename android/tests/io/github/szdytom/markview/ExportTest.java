package io.github.szdytom.markview;

import android.app.Activity;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Color;
import android.net.Uri;
import android.os.SystemClock;
import android.provider.DocumentsContract;
import android.view.KeyEvent;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import org.json.JSONObject;

public class ExportTest extends FolderTestSupport {
    @Override protected void test() throws Exception {
        openReader();
        createFolders();
        Uri tree = DocumentsContract.buildTreeDocumentUri("com.android.externalstorage.documents", "primary:Download/" + directory.getName());
        runOnMainSync(() -> ((MarkviewActivity)activity).onActivityResult(12, Activity.RESULT_OK, new Intent().setData(tree)));
        waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("README.md") && loadedImages(s) == 1);
        if (phone) {
            sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
            waitFor(s -> s.optString("panel").equals("Closed"));
        }
        // Intercept export previews so third-party viewers cannot steal test input.
        android.content.IntentFilter previews = new android.content.IntentFilter(Intent.ACTION_VIEW);
        previews.addDataScheme("content");
        previews.addDataType("application/pdf");
        previews.addDataType("image/png");
        ActivityMonitor preview = addMonitor(previews, null, true);
        File pdf = new File(activity.getFilesDir(), "exports/README.pdf");
        pdf.delete();
        tap("Export");
        waitFor(s -> s.optString("panel").equals("Export"));
        tap("ExportStyles");
        JSONObject exportStyles = waitFor(s -> s.optString("panel").equals("ExportStyles"));
        require(button(exportStyles, "StylesFolder") == null, "Styles folder control is absent from Android export styles");
        tap("ExportStyles");
        waitFor(s -> s.optString("panel").equals("Export"));
        tap("ExportFormat(Pdf)");
        tap("ExportRun");
        waitSystem("documentsui");
        clickText("Save");
        long deadline = SystemClock.uptimeMillis() + 20000;
        while ((!pdf.isFile() || pdf.length() < 100) && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(100);
        require(pdf.isFile() && pdf.length() > 100, "Native PDF export");
        waitFor(s -> preview.getHits() == 1 && s.optBoolean("ready"));
        byte[] bytes = Files.readAllBytes(pdf.toPath());
        require(new String(bytes, 0, 5, StandardCharsets.US_ASCII).equals("%PDF-"), "PDF header");
        String pdfSource = new String(bytes, StandardCharsets.ISO_8859_1);
        require(pdfSource.contains("KaTeX_Main") && pdfSource.contains("KaTeX_Math") && pdfSource.contains("KaTeX_Size"), "PDF embeds bundled KaTeX fonts for inline and display mathematics");
        java.lang.reflect.Field field = MarkviewActivity.class.getDeclaredField("exports");
        field.setAccessible(true);
        @SuppressWarnings("unchecked") java.util.Map<String, Uri> outputs = (java.util.Map<String, Uri>)field.get(activity);
        Uri destination = outputs.get(pdf.getAbsolutePath());
        require(destination != null, "System output URI");
        try (java.io.InputStream input = activity.getContentResolver().openInputStream(destination)) {
            byte[] header = new byte[5]; require(input.read(header) == 5 && new String(header, StandardCharsets.US_ASCII).equals("%PDF-"), "PDF published to selected provider");
        }
        getTargetContext().startActivity(new Intent(getTargetContext(), MarkviewActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        waitFor(s -> s.optString("backend").length() > 0 && s.optBoolean("ready"));
        pass("Shared PDF exporter and Android system save destination");
        File png = new File(activity.getFilesDir(), "exports/README.png"); png.delete();
        tap("Export");
        waitFor(s -> s.optString("panel").equals("Export"));
        tap("ExportFormat(Png)");
        tap("ExportRun");
        waitSystem("documentsui");
        clickText("Save");
        deadline = SystemClock.uptimeMillis() + 25000;
        while ((!png.isFile() || png.length() < 100) && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(100);
        require(png.isFile(), "Native PNG export");
        waitFor(s -> preview.getHits() == 2 && s.optBoolean("ready"));
        removeMonitor(preview);
        Bitmap exported = android.graphics.BitmapFactory.decodeFile(png.getAbsolutePath());
        require(exported != null && exported.getWidth() > 300 && exported.getHeight() > 400, "PNG export dimensions");
        int copper = 0;
        for (int y = 0; y < exported.getHeight(); y += 8) for (int x = 0; x < exported.getWidth(); x += 8) {
            int c = exported.getPixel(x, y);
            if (Color.red(c) > 150 && Color.red(c) > Color.green(c)*1.1 && Color.green(c) > Color.blue(c)*1.1) copper++;
        }
        require(copper > 50, "GPU export contains the local SVG"); exported.recycle();
        pass("Shared PNG exporter and GPU readback with safe-area offsets");
    }
}
