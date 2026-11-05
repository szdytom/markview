package io.github.szdytom.markview;

import android.os.SystemClock;

public class SearchTest extends AndroidTest {
    @Override protected void test() throws Exception {
        openReader();
        tap("SearchOpen");
        waitFor(s -> s.optBoolean("search_open"));
        sendStringSync("needle");
        waitFor(s -> s.optInt("matches") > 0);
        waitFor(s -> keyboardVisible());
        double searchHeight = stableLayout().getJSONArray("dimensions").getDouble(1);
        for (String action : new String[]{"SearchNext", "SearchNext", "SearchPrevious"}) {
            tap(action);
            long deadline = SystemClock.uptimeMillis() + 750;
            do {
                require(keyboardVisible(), "Search navigation keeps the keyboard visible: " + action);
                require(Math.abs(state().getJSONArray("dimensions").getDouble(1) - searchHeight) < 0.1, "Search navigation keeps the viewport height: " + action);
                SystemClock.sleep(30);
            } while (SystemClock.uptimeMillis() < deadline);
        }
        screenshot("search");
        tap("SearchClose");
        waitFor(s -> !s.optBoolean("search_open") && !keyboardVisible());
        pass("Touch search, Android text input and stable keyboard during result navigation");
    }
}
