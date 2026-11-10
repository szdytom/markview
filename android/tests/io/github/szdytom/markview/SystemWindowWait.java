package io.github.szdytom.markview;

/** Waits for a target window, including recovery from a launcher ANR. */
final class SystemWindowWait {
    interface Probe {
        long now();
        void sleep();
        boolean closeLauncherAnr() throws Exception;
        boolean focused();
        void idle(long timeout) throws Exception;
    }

    static void send(Probe probe, String name, Runnable input) throws Exception {
        await(probe, name);
        input.run();
    }

    static void await(Probe probe, String name) throws Exception {
        long deadline = probe.now() + 15000;
        long recoverAfter = 0;
        while (probe.now() < deadline) {
            if (probe.closeLauncherAnr()) {
                // Quickstep restarts asynchronously, after the reader regains focus.
                recoverAfter = probe.now() + 2000;
            } else if (probe.focused()) {
                if (recoverAfter == 0) return;
                if (probe.now() >= recoverAfter) {
                    probe.idle(deadline - probe.now());
                    recoverAfter = 0;
                    // System transitions can change the focused window during `idle`.
                    continue;
                }
            }
            probe.sleep();
        }
        throw new AssertionError("System window: " + name);
    }
}
