package io.github.szdytom.markview;

import java.util.concurrent.TimeoutException;

/** Virtual time covers launcher recovery without an emulator. */
public final class SystemWindowWaitTest {
    private static final class Probe implements SystemWindowWait.Probe {
        long time;
        long anrAt = -1;
        long secondAnrAt = -1;
        long focusAt;
        long busyUntil;
        boolean loseFocusOnIdle;
        int closed;
        int idled;

        public long now() { return time; }
        public void sleep() { time += 100; }
        public boolean focused() { return time >= focusAt; }
        public boolean closeLauncherAnr() {
            if (anrAt < 0 || time < anrAt) return false;
            closed++;
            anrAt = secondAnrAt;
            secondAnrAt = -1;
            return true;
        }
        public void idle(long timeout) throws TimeoutException {
            idled++;
            long until = Math.max(time, busyUntil) + 1000;
            if (until - time > timeout) throw new TimeoutException("System stayed busy");
            time = until;
            if (loseFocusOnIdle) focusAt = time + 500;
        }
    }

    public static void main(String[] args) throws Exception {
        Probe normal = new Probe();
        SystemWindowWait.await(normal, "reader");
        assert normal.time == 0 && normal.idled == 0 : "Normal taps need no recovery delay";

        int[] keys = {0};
        SystemWindowWait.send(normal, "reader", () -> keys[0]++);
        assert keys[0] == 1 && normal.time == 0 : "Focused input is sent once without a delay";

        Probe blockedBack = new Probe();
        blockedBack.anrAt = 0;
        blockedBack.busyUntil = 2500;
        SystemWindowWait.send(blockedBack, "reader", () -> {
            assert blockedBack.closed == 1 && blockedBack.idled == 1 && blockedBack.time == 3500
                : "Back must wait for the launcher ANR to close and the system to recover";
            keys[0]++;
        });
        assert keys[0] == 2 : "Recovery sends Back exactly once";

        Probe picker = new Probe();
        picker.focusAt = 500;
        SystemWindowWait.send(picker, "documentsui", () -> {
            assert picker.time == 500 : "Picker cancellation waits for the picker window";
            keys[0]++;
        });

        Probe restart = new Probe();
        restart.anrAt = 0;
        restart.busyUntil = 2500;
        SystemWindowWait.await(restart, "reader");
        assert restart.time == 3500 && restart.closed == 1 && restart.idled == 1
            : "Reader focus must not bypass delayed launcher startup and system activity";

        Probe repeated = new Probe();
        repeated.anrAt = 0;
        repeated.secondAnrAt = 1500;
        SystemWindowWait.await(repeated, "reader");
        assert repeated.time == 4500 && repeated.closed == 2 : "A second ANR restarts recovery";

        Probe changedWindow = new Probe();
        changedWindow.anrAt = 0;
        changedWindow.loseFocusOnIdle = true;
        SystemWindowWait.await(changedWindow, "documentsui");
        assert changedWindow.time == 3500 : "Recheck target focus after system idle";

        Probe lateAnr = new Probe();
        lateAnr.focusAt = 14900;
        lateAnr.anrAt = 14900;
        try {
            SystemWindowWait.await(lateAnr, "reader");
            throw new AssertionError("A late ANR must not extend the deadline");
        } catch (AssertionError error) {
            assert error.getMessage().equals("System window: reader") : error;
            assert lateAnr.time == 15000;
        }

        Probe busy = new Probe();
        busy.anrAt = 0;
        busy.busyUntil = 15000;
        try {
            SystemWindowWait.await(busy, "reader");
            throw new AssertionError("System idle timeout must fail the test");
        } catch (TimeoutException expected) {
            assert busy.idled == 1;
        }

        Probe blocked = new Probe();
        blocked.anrAt = 0;
        blocked.busyUntil = 15000;
        try {
            SystemWindowWait.send(blocked, "reader", () -> keys[0]++);
            throw new AssertionError("A busy system must prevent key injection");
        } catch (TimeoutException expected) {
            assert keys[0] == 3 : "An idle timeout must not send input";
        }

        Probe missingWindow = new Probe();
        missingWindow.focusAt = Long.MAX_VALUE;
        try {
            SystemWindowWait.await(missingWindow, "documentsui");
            throw new AssertionError("A missing target must time out");
        } catch (AssertionError error) {
            assert error.getMessage().equals("System window: documentsui") : error;
            assert missingWindow.time == 15000;
        }

        try {
            SystemWindowWait.send(missingWindow, "reader", () -> keys[0]++);
            throw new AssertionError("A missing window must prevent key injection");
        } catch (AssertionError error) {
            assert error.getMessage().equals("System window: reader") : error;
            assert keys[0] == 3 : "Failed recovery must not send input to another window";
        }
    }
}
