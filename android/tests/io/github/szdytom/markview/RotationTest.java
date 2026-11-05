package io.github.szdytom.markview;

import android.os.SystemClock;
import android.view.KeyEvent;
import org.json.JSONArray;
import org.json.JSONObject;

public class RotationTest extends AndroidTest {
    @Override protected void test() throws Exception {
        openReader();
        getUiAutomation().setRotation(phone ? 1 : 1 - portraitRotation);
        if (phone) {
            SystemClock.sleep(1000);
            JSONObject rotated = state();
            JSONArray size = rotated.getJSONArray("dimensions");
            require(size.getDouble(0) < size.getDouble(1) && rotated.getBoolean("phone_layout"), "Phone remains portrait when the device rotates");
            tap("Tabs");
            sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
            waitFor(s -> s.optString("panel").equals("Closed"));
            pass("Phone portrait lock and Back dismissal of the tab drawer");
        } else {
            waitFor(s -> s.optJSONArray("dimensions").optDouble(0) > s.optJSONArray("dimensions").optDouble(1));
            require(!state().getBoolean("phone_layout"), "Tablet remains a tablet in landscape");
            screenshot("landscape");
            tap("Settings");
            waitFor(s -> s.optString("panel").equals("Settings(Generic)"));
            requireSettingsLayout();
            requireTabletColumnWidthControls();
            for (int i = 0; i < 9; i++) tap("Wider");
            waitFor(s -> s.optDouble("column_width") >= 760);
            screenshot("landscape-settings");
            sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
            waitFor(s -> s.optString("panel").equals("Closed"));
            stableLayout();
            screenshot("tablet-tabs");
            pass("Tablet rotation and centered settings dialog");
        }
        getUiAutomation().setRotation(portraitRotation);
        waitFor(s -> s.optJSONArray("dimensions").optDouble(0) < s.optJSONArray("dimensions").optDouble(1));
    }
}
