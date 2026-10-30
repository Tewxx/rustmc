package ft.bus;

import ft.Trace;
import java.util.*;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.fml.common.FMLCommonHandler;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.FMLInitializationEvent;
import net.minecraftforge.fml.common.eventhandler.*;

/**
 * EV-01..EV-10: EventBus semantics, driven synchronously from FMLInitializationEvent (no world needed):
 * priority order, cancel / receiveCanceled, Result, register/unregister, static vs instance handlers,
 * superclass delivery, Forge bus vs FML bus, handler exceptions, error cases.
 */
@Mod(modid = "ftbus", name = "FT Bus", version = "1.0", dependencies = "required-after:ftcommon",
     acceptableRemoteVersions = "*", clientSideOnly = true)
public class FTBus {
    static final Trace.T T = Trace.of("bus");

    @Cancelable
    public static class Cancelable1 extends Event {
        public final String name;
        public Cancelable1(String name) { this.name = name; }
    }

    @Event.HasResult
    public static class Resulty extends Event {}

    public static class Plain extends Event {}
    public static class SubPlain extends Plain {}

    /** Handlers in deliberately shuffled registration order; the bus must sort by priority. */
    public static class Priorities {
        @SubscribeEvent(priority = EventPriority.LOW) public void low(Plain e) { T.log("EV-01", "handler", "name", "LOW"); }
        @SubscribeEvent(priority = EventPriority.HIGHEST) public void highest(Plain e) { T.log("EV-01", "handler", "name", "HIGHEST"); }
        @SubscribeEvent public void normal(Plain e) { T.log("EV-01", "handler", "name", "NORMAL"); }
        @SubscribeEvent(priority = EventPriority.LOWEST) public void lowest(Plain e) { T.log("EV-01", "handler", "name", "LOWEST"); }
        @SubscribeEvent(priority = EventPriority.HIGH) public void high(Plain e) { T.log("EV-01", "handler", "name", "HIGH"); }
    }

    public static class Cancelers {
        @SubscribeEvent(priority = EventPriority.HIGH) public void canceller(Cancelable1 e) {
            T.log("EV-02", "handler", "name", "HIGH-canceller", "wasCanceled", e.isCanceled());
            e.setCanceled(true);
        }
        @SubscribeEvent public void normal(Cancelable1 e) { T.log("EV-02", "handler", "name", "NORMAL-should-not-run"); }
        @SubscribeEvent(priority = EventPriority.LOW, receiveCanceled = true) public void receiver(Cancelable1 e) {
            T.log("EV-02", "handler", "name", "LOW-receiveCanceled", "isCanceled", e.isCanceled());
        }
        @SubscribeEvent(priority = EventPriority.LOWEST, receiveCanceled = true) public void uncancel(Cancelable1 e) {
            e.setCanceled(false);
            T.log("EV-02", "handler", "name", "LOWEST-uncancel", "isCanceled", e.isCanceled());
        }
    }

    public static class ResultHandlers {
        @SubscribeEvent(priority = EventPriority.HIGH) public void a(Resulty e) {
            T.log("EV-03", "handler", "name", "HIGH", "resultBefore", e.getResult());
            e.setResult(Event.Result.ALLOW);
        }
        @SubscribeEvent(priority = EventPriority.LOW) public void b(Resulty e) {
            T.log("EV-03", "handler", "name", "LOW", "resultSeen", e.getResult());
            e.setResult(Event.Result.DENY);
        }
    }

    public static class Counter {
        int n;
        @SubscribeEvent public void on(Plain e) { n++; }
    }

    public static class Statics {
        @SubscribeEvent public static void staticHandler(Plain e) { T.log("EV-05", "handler", "name", "static"); }
        @SubscribeEvent public void instanceHandler(Plain e) { T.log("EV-05", "handler", "name", "instance"); }
    }

    public static class Supers {
        @SubscribeEvent public void onPlain(Plain e) { T.log("EV-06", "handler", "name", "onPlain", "got", e.getClass().getSimpleName()); }
        @SubscribeEvent public void onSub(SubPlain e) { T.log("EV-06", "handler", "name", "onSub", "got", e.getClass().getSimpleName()); }
        @SubscribeEvent public void onBase(Event e) { if (e instanceof Plain) T.log("EV-06", "handler", "name", "onEvent(base)", "got", e.getClass().getSimpleName()); }
    }

    public static class FmlSide {
        @SubscribeEvent public void on(Plain e) { T.log("EV-07", "handler", "bus", "FML"); }
    }

    public static class ForgeSide {
        @SubscribeEvent public void on(Plain e) { T.log("EV-07", "handler", "bus", "FORGE"); }
    }

    public static class Thrower {
        @SubscribeEvent(priority = EventPriority.HIGH) public void a(Plain e) { T.log("EV-08", "handler", "name", "HIGH-throws"); throw new IllegalStateException("boom"); }
        @SubscribeEvent(priority = EventPriority.LOW) public void b(Plain e) { T.log("EV-08", "handler", "name", "LOW-should-not-run"); }
    }

    public static class BadArity {
        @SubscribeEvent public void two(Plain a, Plain b) {}
    }

    public static class BadType {
        @SubscribeEvent public void notEvent(String s) {}
    }

    public static class NotAnnotated {
        public void on(Plain e) { T.log("EV-10", "handler", "name", "should-not-run"); }
    }

    private static String post(EventBus bus, Event e) {
        try {
            boolean r = bus.post(e);
            return String.valueOf(r);
        } catch (Throwable t) {
            return "threw:" + t.getClass().getName() + ":" + String.valueOf(t.getMessage()).replaceAll("[^A-Za-z0-9 :._-]", "?");
        }
    }

    @Mod.EventHandler
    public void init(FMLInitializationEvent ev) {
        EventBus forge = MinecraftForge.EVENT_BUS;
        EventBus fml = FMLCommonHandler.instance().bus();

        Priorities p = new Priorities();
        forge.register(p);
        T.log("EV-01", "post", "returned", post(forge, new Plain()));
        forge.unregister(p);

        Cancelers c = new Cancelers();
        forge.register(c);
        Cancelable1 ce = new Cancelable1("c");
        T.log("EV-02", "post", "returned", post(forge, ce), "finalCanceled", ce.isCanceled(), "isCancelable", ce.isCancelable());
        forge.unregister(c);

        ResultHandlers rh = new ResultHandlers();
        forge.register(rh);
        Resulty r = new Resulty();
        T.log("EV-03", "post", "returned", post(forge, r), "hasResult", r.hasResult(), "finalResult", r.getResult());
        forge.unregister(rh);
        T.log("EV-03", "default", "result", new Resulty().getResult(), "hasResult", new Resulty().hasResult(), "plainHasResult", new Plain().hasResult());

        Plain plain = new Plain();
        try { plain.setCanceled(true); } catch (Throwable t) { T.log("EV-02", "setCanceled-on-uncancelable", "threw", t.getClass().getName()); }
        T.log("EV-02", "plain", "isCancelable", plain.isCancelable(), "isCanceled", plain.isCanceled());

        Counter k = new Counter();
        forge.register(k);
        forge.register(k); // double register must not double deliver
        post(forge, new Plain());
        int afterFirst = k.n;
        forge.unregister(k);
        post(forge, new Plain());
        forge.register(k);
        post(forge, new Plain());
        forge.unregister(k);
        forge.unregister(k); // unregister of unregistered is a no-op
        T.log("EV-04", "register-unregister", "afterFirstPost", afterFirst, "afterUnregister", afterFirst == k.n - 1 ? "ok" : "n=" + k.n, "total", k.n);

        Statics s = new Statics();
        forge.register(s);
        post(forge, new Plain());
        forge.unregister(s);
        try { forge.register(Statics.class); post(forge, new Plain()); forge.unregister(Statics.class); T.log("EV-05", "register(Class)", "ok", true); }
        catch (Throwable t) { T.log("EV-05", "register(Class)", "threw", t.getClass().getName()); }

        Supers su = new Supers();
        forge.register(su);
        post(forge, new SubPlain());
        post(forge, new Plain());
        forge.unregister(su);

        FmlSide fs = new FmlSide();
        ForgeSide gs = new ForgeSide();
        fml.register(fs);
        forge.register(gs);
        T.log("EV-07", "post-forge", "returned", post(forge, new Plain()));
        T.log("EV-07", "post-fml", "returned", post(fml, new Plain()));
        T.log("EV-07", "distinct", "same", forge == fml);
        fml.unregister(fs);
        forge.unregister(gs);

        Thrower th = new Thrower();
        forge.register(th);
        T.log("EV-08", "post", "returned", post(forge, new Plain()));
        forge.unregister(th);

        T.log("EV-09", "bad-arity", "result", tryRegister(forge, new BadArity()));
        T.log("EV-09", "bad-type", "result", tryRegister(forge, new BadType()));
        NotAnnotated na = new NotAnnotated();
        forge.register(na);
        post(forge, new Plain());
        forge.unregister(na);
        T.log("EV-10", "done");
    }

    private static String tryRegister(EventBus bus, Object o) {
        try { bus.register(o); return "registered"; } catch (Throwable t) { return "threw:" + t.getClass().getName(); }
    }
}
