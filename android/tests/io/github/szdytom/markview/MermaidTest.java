package io.github.szdytom.markview;

import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Color;
import java.io.File;

public class MermaidTest extends AndroidTest {
    @Override protected void test() throws Exception {
        openReader();
        checkMermaidLabels("mermaid-light");
        selectDarkStyle();
        checkMermaidLabels("mermaid-dark");
    }
    private void checkMermaidLabels(String name) throws Exception {
        getTargetContext().startActivity(intent("mermaid.md", Intent.ACTION_VIEW));
        waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("/mermaid.md") && loadedImages(s) == 1);
        stableLayout();
        screenshot(name);
        File capture = new File(activity.getFilesDir(), "test-artifacts/" + name + ".png");
        Bitmap pixels = android.graphics.BitmapFactory.decodeFile(capture.getAbsolutePath());
        int[] ink = new int[3];
        for (int y = 0; y < pixels.getHeight(); y++) for (int x = 0; x < pixels.getWidth(); x++) {
            int c = pixels.getPixel(x, y);
            int r = Color.red(c), g = Color.green(c), b = Color.blue(c);
            if (r > 200 && g < 50 && b < 50) ink[0]++;
            if (g > 200 && r < 50 && b < 50) ink[1]++;
            if (b > 200 && r < 50 && g < 50) ink[2]++;
        }
        pixels.recycle();
        require(ink[0] > 10 && ink[1] > 10 && ink[2] > 10, "All three Mermaid labels render in " + name);
        pass("Mermaid label pixels in " + name);
    }
}
