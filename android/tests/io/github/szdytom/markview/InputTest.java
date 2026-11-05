package io.github.szdytom.markview;

import android.app.Activity;
import android.os.SystemClock;
import android.view.InputDevice;
import android.view.KeyEvent;
import android.view.MotionEvent;
import org.json.JSONArray;
import org.json.JSONObject;

public class InputTest extends AndroidTest {
    @Override protected void test() throws Exception {
        openReader();
        tap("SearchOpen");
        waitFor(s -> s.optBoolean("search_open") && keyboardVisible());
        commitInput("中文🙂");
        waitFor(s -> s.optString("search_query").equals("中文🙂"));
        runOnMainSync(() -> {
            android.view.inputmethod.InputConnection connection = activity.getCurrentFocus().onCreateInputConnection(new android.view.inputmethod.EditorInfo());
            connection.sendKeyEvent(new KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DEL));
            connection.sendKeyEvent(new KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_DEL));
        });
        waitFor(s -> s.optString("search_query").equals("中文"));
        commitInput("x");
        waitFor(s -> s.optString("search_query").equals("中文x"));
        runOnMainSync(() -> activity.getCurrentFocus().onCreateInputConnection(new android.view.inputmethod.EditorInfo()).deleteSurroundingText(1, 0));
        waitFor(s -> s.optString("search_query").equals("中文"));
        commitInput("🙂");
        waitFor(s -> s.optString("search_query").equals("中文🙂"));
        runOnMainSync(() -> activity.getCurrentFocus().onCreateInputConnection(new android.view.inputmethod.EditorInfo()).deleteSurroundingTextInCodePoints(1, 0));
        waitFor(s -> s.optString("search_query").equals("中文"));
        commitInput("🙂");
        waitFor(s -> s.optString("search_query").equals("中文🙂"));
        JSONObject caretLayout = stableLayout();
        JSONArray caretRect = caretLayout.getJSONArray("search_rect");
        float caretLeft = (float)caretRect.getDouble(0) + 8;
        float caretRight = (float)(caretRect.getDouble(0) + caretRect.getDouble(2) - 8);
        float caretY = (float)(caretRect.getDouble(1) + caretRect.getDouble(3) / 2);
        swipe(caretLeft, caretY, caretLeft, caretY);
        commitInput("A");
        waitFor(s -> s.optString("search_query").equals("A中文🙂"));
        swipe(caretRight, caretY, caretLeft, caretY);
        commitInput("B");
        waitFor(s -> s.optString("search_query").equals("BA中文🙂"));
        swipe(caretRight, caretY, caretRight, caretY);
        commitInput("Z");
        waitFor(s -> s.optString("search_query").equals("BA中文🙂Z") && keyboardVisible());
        pass("Touch taps and drags reposition the caret without selecting text or closing the keyboard");
        tap("SearchClose");
        tap("SearchOpen");
        waitFor(s -> s.optBoolean("search_open") && keyboardVisible());
        stableLayout();
        runOnMainSync(() -> ((android.content.ClipboardManager)activity.getSystemService(Activity.CLIPBOARD_SERVICE))
            .setPrimaryClip(android.content.ClipData.newPlainText("Markview test", "Markview renders")));
        JSONObject current = state();
        JSONArray rect = current.getJSONArray("search_rect"), insets = current.getJSONArray("insets");
        double scale = current.getJSONArray("dimensions").getDouble(2);
        int[] origin = new int[2];
        runOnMainSync(() -> activity.getWindow().getDecorView().getLocationOnScreen(origin));
        float x = (float)((rect.getDouble(0) + rect.getDouble(2) / 2 + insets.getDouble(0)) * scale + origin[0]);
        float y = (float)((rect.getDouble(1) + rect.getDouble(3) / 2 + insets.getDouble(1)) * scale + origin[1]);
        long down = SystemClock.uptimeMillis();
        MotionEvent press = MotionEvent.obtain(down, down, MotionEvent.ACTION_DOWN, x, y, 0);
        press.setSource(InputDevice.SOURCE_TOUCHSCREEN);
        getUiAutomation().injectInputEvent(press, true);
        press.recycle();
        SystemClock.sleep(800);
        MotionEvent release = MotionEvent.obtain(down, SystemClock.uptimeMillis(), MotionEvent.ACTION_UP, x, y, 0);
        release.setSource(InputDevice.SOURCE_TOUCHSCREEN);
        getUiAutomation().injectInputEvent(release, true);
        release.recycle();
        clickText(activity.getString(android.R.string.paste));
        waitFor(s -> s.optString("search_query").equals("Markview renders") && s.optInt("matches") > 0 && keyboardVisible());
        runOnMainSync(() -> {
            android.view.inputmethod.InputConnection connection = activity.getCurrentFocus().onCreateInputConnection(new android.view.inputmethod.EditorInfo());
            for (int i = 0; i < "Markview renders".length(); i++) connection.deleteSurroundingText(1, 0);
        });
        waitFor(s -> s.optString("search_query").isEmpty() && s.optInt("matches") == 0);
        tap("SearchClose");
        waitFor(s -> !s.optBoolean("search_open"));
        pass("IME clipboard commits and deletion preserve Unicode; long-press Paste replaces the selection and updates search");
    }

}
