package io.github.szdytom.markview;

import android.app.Activity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Intent;
import android.os.SystemClock;
import android.view.InputDevice;
import android.view.KeyCharacterMap;
import android.view.KeyEvent;
import java.util.concurrent.atomic.AtomicReference;

public class KeyboardTest extends AndroidTest {
    @Override protected void test() throws Exception {
        activity = startActivitySync(intent("reader.md", Intent.ACTION_VIEW));
        waitFor(s -> s.optBoolean("ready") && s.optInt("blocks") > 20);
        stableLayout();
        pressKey(KeyEvent.KEYCODE_PAGE_DOWN);
        waitFor(s -> s.optDouble("scroll") > 100);
        pressKey(KeyEvent.KEYCODE_PAGE_UP);
        waitFor(s -> s.optDouble("scroll") < 1);
        chord(KeyEvent.KEYCODE_CTRL_LEFT, KeyEvent.KEYCODE_B);
        waitFor(s -> s.optBoolean("outline_open"));
        chord(KeyEvent.KEYCODE_CTRL_RIGHT, KeyEvent.KEYCODE_B);
        waitFor(s -> !s.optBoolean("outline_open"));

        clipboard("sentinel");
        chord(KeyEvent.KEYCODE_CTRL_LEFT, KeyEvent.KEYCODE_A);
        chord(KeyEvent.KEYCODE_CTRL_LEFT, KeyEvent.KEYCODE_C);
        waitFor(s -> clipboard().contains("Markview renders this document"));
        int tabs = state().getJSONArray("tabs").length();
        clipboard("# Keyboard paste\n\nPasted with Ctrl+V.");
        chord(KeyEvent.KEYCODE_CTRL_RIGHT, KeyEvent.KEYCODE_V);
        waitFor(s -> s.optJSONArray("tabs").length() == tabs + 1 && s.optBoolean("ready"));

        chord(KeyEvent.KEYCODE_CTRL_LEFT, KeyEvent.KEYCODE_F);
        waitFor(s -> s.optBoolean("search_open") && keyboardVisible());
        clipboard("Keyboard");
        chord(KeyEvent.KEYCODE_CTRL_RIGHT, KeyEvent.KEYCODE_V);
        waitFor(s -> s.optString("search_query").equals("Keyboard") && s.optInt("matches") > 0);
        chord(KeyEvent.KEYCODE_CTRL_LEFT, KeyEvent.KEYCODE_A);
        clipboard("sentinel");
        chord(KeyEvent.KEYCODE_CTRL_LEFT, KeyEvent.KEYCODE_C);
        waitFor(s -> clipboard().equals("Keyboard"));
        clipboard("replaced");
        chord(KeyEvent.KEYCODE_CTRL_LEFT, KeyEvent.KEYCODE_V);
        waitFor(s -> s.optString("search_query").equals("replaced"));
        commitInput("x");
        waitFor(s -> s.optString("search_query").equals("replacedx"));
        pass("External keyboard paging, Contents, document copy/paste and search shortcuts work with either Ctrl key");
    }

    private void chord(int modifier, int code) {
        int meta = KeyEvent.META_CTRL_ON | (modifier == KeyEvent.KEYCODE_CTRL_LEFT
            ? KeyEvent.META_CTRL_LEFT_ON : KeyEvent.META_CTRL_RIGHT_ON);
        long down = SystemClock.uptimeMillis();
        inject(down, KeyEvent.ACTION_DOWN, modifier, meta);
        inject(down, KeyEvent.ACTION_DOWN, code, meta);
        inject(down, KeyEvent.ACTION_UP, code, meta);
        inject(down, KeyEvent.ACTION_UP, modifier, 0);
    }

    private void inject(long down, int action, int code, int meta) {
        require(getUiAutomation().injectInputEvent(new KeyEvent(down, SystemClock.uptimeMillis(),
            action, code, 0, meta, KeyCharacterMap.VIRTUAL_KEYBOARD, 0, 0, InputDevice.SOURCE_KEYBOARD), true),
            "Keyboard event reaches the reader");
    }

    private String clipboard() {
        AtomicReference<String> text = new AtomicReference<>();
        runOnMainSync(() -> text.set(((MarkviewActivity)activity).pasteText()));
        return text.get();
    }

    private void clipboard(String text) {
        runOnMainSync(() -> ((ClipboardManager)activity.getSystemService(Activity.CLIPBOARD_SERVICE))
            .setPrimaryClip(ClipData.newPlainText("Markview test", text)));
    }
}
