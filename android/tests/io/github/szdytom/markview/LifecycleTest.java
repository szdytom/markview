package io.github.szdytom.markview;

import android.app.Activity;
import android.content.Intent;
import android.os.Environment;
import android.os.SystemClock;
import java.io.File;
import org.json.JSONArray;
import org.json.JSONObject;

public class LifecycleTest extends AndroidTest {
    @Override protected void test() throws Exception {
        openReader();
        getTargetContext().startActivity(intent("second.md", Intent.ACTION_VIEW));
        waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("second.md"));
        tap("Settings");
        waitFor(s -> s.optString("panel").equals("Settings(Generic)"));
        for (int i = 0; i < 8 && button(state(), "Larger") == null; i++) {
            swipe(180, 550, 180, 270);
            swipe(180, 223, 180, 223);
        }
        double fontSize = state().getDouble("font_size");
        tap("Larger");
        waitFor(s -> s.optDouble("font_size") > fontSize);
        sendKeyDownUpSync(android.view.KeyEvent.KEYCODE_BACK);
        waitFor(s -> s.optString("panel").equals("Closed"));
        selectDarkStyle();
        checkLifecycle();
    }
    private void checkLifecycle() throws Exception {
        getTargetContext().startActivity(intent("reader.md", Intent.ACTION_VIEW));
        waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("reader.md") && loadedImages(s) >= 2);
        org.json.JSONArray retained = new org.json.JSONArray();
        org.json.JSONArray openedTabs = state().getJSONArray("tabs");
        for (int i = 0; i < openedTabs.length(); i++) {
            String path = openedTabs.getString(i);
            if (path.contains("/files/documents/") || path.contains("/files/shared/")
                || path.startsWith(Environment.getExternalStorageDirectory().getPath() + "/")
                || path.startsWith("http://") || path.startsWith("https://")) retained.put(path);
        }
        int pid = android.os.Process.myPid();
        double fontSize = state().getDouble("font_size");
        for (int attempt = 0; attempt < 4; attempt++) {
            boolean recreate = attempt < 2;
            boolean discardOnBackground = attempt == 3;
            int alwaysFinish = android.provider.Settings.Global.getInt(getTargetContext().getContentResolver(), "always_finish_activities", 0);
            if (discardOnBackground) alwaysFinish(true);
            Activity previous = activity;
            ActivityMonitor monitor = addMonitor(MarkviewActivity.class.getName(), null, false);
            try {
                runOnMainSync(() -> {
                    if (recreate) previous.recreate();
                    else if (discardOnBackground) previous.startActivity(new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
                    else previous.finish();
                });
                long deadline = SystemClock.uptimeMillis() + 5000;
                while (!previous.isDestroyed() && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(50);
                require(previous.isDestroyed(), "Activity destruction finishes within five seconds");
                if (!recreate) activity = startActivitySync(intent("reader.md", Intent.ACTION_VIEW));
                else activity = monitor.waitForActivityWithTimeout(10000);
                require(activity != null && activity != previous, "A new Activity is created in the same process");
                JSONObject recreated = waitFor(s -> s.optBoolean("ready") && s.optInt("blocks") > 20 && loadedImages(s) >= 2);
                require(android.os.Process.myPid() == pid, "Activity restart preserves the process");
                require(recreated.getDouble("font_size") == fontSize && recreated.optString("selected_styles").contains("dark"), "Activity restart retains preferences");
                require(recreated.getJSONArray("tabs").toString().equals(retained.toString()), "Activity restart restores the existing tab session");
                java.lang.reflect.Method deliver = MarkviewActivity.class.getDeclaredMethod("deliver", int.class, String.class);
                deliver.setAccessible(true);
                deliver.invoke(previous, 0, new File(previous.getFilesDir(), "stale.md").getAbsolutePath());
                SystemClock.sleep(250);
                require(state().getJSONArray("tabs").toString().equals(retained.toString()), "Destroyed Activity cannot deliver stale I/O results");
            } finally {
                removeMonitor(monitor);
                if (discardOnBackground) {
                    alwaysFinish(alwaysFinish != 0);
                }
            }
        }
        pass("Activity recreation, finish/reopen and Don't keep activities preserve preferences without stale callbacks");
    }
}
