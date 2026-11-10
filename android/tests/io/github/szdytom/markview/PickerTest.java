package io.github.szdytom.markview;

import android.os.SystemClock;
import android.view.KeyEvent;

public class PickerTest extends AndroidTest {
    @Override protected void test() throws Exception {
        openReader();
        // The toolbar has no open button: phones open the drawer, while the
        // tablet drives the picker directly so the reader stays active.
        if (phone) {
            tap("Tabs");
            tap("Open");
        } else {
            runOnMainSync(() -> ((MarkviewActivity)activity).pickDocument(""));
        }
        SystemClock.sleep(350);
        pressKey(KeyEvent.KEYCODE_BACK, "documentsui");
        waitFor(s -> s.optString("backend").length() > 0 && s.optJSONArray("tabs").length() == 1 && s.optString("panel").equals("Closed"));
        waitSystem(getTargetContext().getPackageName());
        stableLayout();
        pass("Android Storage Access Framework picker and cancellation");
        runOnMainSync(() -> ((MarkviewActivity)activity).pickFolder(""));
        pressKey(KeyEvent.KEYCODE_BACK, "documentsui");
        waitFor(s -> s.optString("backend").length() > 0);
        waitSystem(getTargetContext().getPackageName());
        stableLayout();
    }
}
