package io.github.szdytom.markview;

public class ReaderFailureTest extends BridgeTest {
    @Override protected void test() throws Exception {
        checkFatal(4, "Injected reader startup failure");
    }
}
