package io.github.szdytom.markview;

import android.os.SystemClock;

public class ScrollTest extends AndroidTest {
    @Override protected void test() throws Exception {
        openReader();
        swipe(180, 590, 180, 180);
        waitFor(s -> s.optDouble("scroll") > 100);
        SystemClock.sleep(1200);
        double down = state().getDouble("scroll");
        require(down > 200, "Drag and release inertia move the document");
        swipe(180, 180, 180, 590);
        waitFor(s -> s.optDouble("scroll") < down - 100);
        SystemClock.sleep(1200);
        double up = state().getDouble("scroll");
        require(up < down - 200, "Reverse drag and inertia follow the finger");
        swipe(180, 590, 180, 180);
        waitFor(s -> s.optDouble("scroll") > up + 100);
        SystemClock.sleep(1200);
        screenshot("scroll");
        pass("Touch drag, reverse drag and release inertia render the reader");
    }
}
