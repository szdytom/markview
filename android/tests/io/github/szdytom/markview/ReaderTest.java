package io.github.szdytom.markview;

import org.json.JSONObject;

public class ReaderTest extends AndroidTest {
    @Override protected void test() throws Exception {
        JSONObject initial = openReader();
        require(initial.getInt("math_errors") == 0, "Mathematics layout");
        require(MarkviewActivity.nativeBridgeReferencesReleased(), "JNI local references are released on success and failure");
        pass("JNI bridge releases local references while the calling thread remains attached");
        require(initial.getString("backend").equals("Vulkan") || initial.getString("backend").equals("Gl"), "GPU backend");
        screenshot("reader");
        checkToolbar();
        checkSystemBars(true);
        swipe(280, 180, 100, 180);
        waitFor(s -> s.optBoolean("outline_open"));
        swipe(280, 180, 100, 180);
        require(state().getBoolean("outline_open"), "Left swipe keeps Contents open");
        tap("Outline");
        waitFor(s -> !s.optBoolean("outline_open"));
        swipe(180, 590, 180, 180);
        waitFor(s -> s.optDouble("scroll") > 100);
        pass("Markdown, CJK, math, code, Mermaid, images and touch scroll");
    }
}
