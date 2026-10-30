package rustmc.forge;

import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.util.ArrayList;
import java.util.List;
import net.minecraftforge.fml.common.eventhandler.Event;
import net.minecraftforge.fml.common.eventhandler.EventPriority;
import net.minecraftforge.fml.common.eventhandler.SubscribeEvent;

/** The one bus behind MinecraftForge.EVENT_BUS and FMLCommonHandler.bus(); EventBus.register/post forward here. */
public final class Bus {
    private static final class Listener {
        final Object target; final Method method; final EventPriority priority; final boolean receiveCanceled;
        Listener(Object target, Method method, SubscribeEvent a) {
            this.target = target; this.method = method; priority = a.priority(); receiveCanceled = a.receiveCanceled();
            method.setAccessible(true);
        }
    }

    private static final List<Listener> listeners = new ArrayList<>();

    /** An object registers its @SubscribeEvent methods; a Class registers its static ones. */
    public static synchronized void register(Object target) {
        boolean statics = target instanceof Class;
        for (Method m : (statics ? (Class<?>) target : target.getClass()).getMethods()) {
            SubscribeEvent a = m.getAnnotation(SubscribeEvent.class);
            if (a == null || m.getParameterTypes().length != 1 || !Event.class.isAssignableFrom(m.getParameterTypes()[0])) continue;
            if (statics && !Modifier.isStatic(m.getModifiers())) continue;
            Listener l = new Listener(statics ? null : target, m, a);
            int i = listeners.size();
            while (i > 0 && listeners.get(i - 1).priority.ordinal() > l.priority.ordinal()) i--;
            listeners.add(i, l);
        }
    }

    public static synchronized void unregister(Object target) {
        for (int i = listeners.size() - 1; i >= 0; i--) if (listeners.get(i).target == target) listeners.remove(i);
    }

    private static final java.util.Map<Class<?>, boolean[]> traits = new java.util.concurrent.ConcurrentHashMap<>();

    /** {cancelable, hasResult} from @Cancelable / @HasResult on the event class or an ancestor. */
    private static boolean[] traits(Class<?> c) {
        boolean[] t = traits.get(c);
        if (t == null) {
            t = new boolean[2];
            for (Class<?> k = c; k != null && k != Event.class; k = k.getSuperclass()) {
                t[0] |= k.isAnnotationPresent(net.minecraftforge.fml.common.eventhandler.Cancelable.class);
                t[1] |= k.isAnnotationPresent(Event.HasResult.class);
            }
            traits.put(c, t);
        }
        return t;
    }

    public static boolean cancelable(Event e) { return traits(e.getClass())[0]; }
    public static boolean hasResult(Event e) { return traits(e.getClass())[1]; }

    /** Returns true if a listener cancelled the event. */
    public static boolean post(Event e) {
        Listener[] snapshot;
        synchronized (Bus.class) { snapshot = listeners.toArray(new Listener[0]); }
        for (Listener l : snapshot) {
            if (!l.method.getParameterTypes()[0].isInstance(e)) continue;
            if (cancelable(e) && e.isCanceled() && !l.receiveCanceled) continue;
            try {
                l.method.invoke(l.target, e);
            } catch (Throwable t) {
                Throwable cause = t instanceof java.lang.reflect.InvocationTargetException ? t.getCause() : t;
                System.err.println("[forge] exception in " + l.method + " handling " + e.getClass().getName());
                cause.printStackTrace();
                throw new RuntimeException(cause);
            }
        }
        return cancelable(e) && e.isCanceled();
    }
}
