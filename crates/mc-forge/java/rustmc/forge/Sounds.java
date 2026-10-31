package rustmc.forge;

import java.lang.reflect.Field;
import java.util.Collections;
import java.util.HashMap;
import java.util.Map;
import java.util.Set;
import java.util.WeakHashMap;
import net.minecraft.client.Minecraft;
import net.minecraft.client.audio.ISound;
import net.minecraft.client.audio.PositionedSoundRecord;
import net.minecraft.client.audio.SoundCategory;
import net.minecraft.client.audio.SoundEventAccessorComposite;
import net.minecraft.client.audio.SoundHandler;
import net.minecraft.client.audio.SoundManager;
import net.minecraft.client.audio.SoundPoolEntry;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.util.ResourceLocation;

/**
 * Everything about sound that Java code can see: a constructed {@code SoundHandler}/{@code SoundManager}
 * pair, the sound-event registry, the set of sounds playing right now and the fields of every
 * {@code PositionedSoundRecord}. The stub constructors never run, so objects allocated by Rust have to be
 * filled in here. Playback requests themselves are queued in Rust ({@code sound::take_sounds}).
 */
public final class Sounds {
    private static final Map<String, SoundEventAccessorComposite> EVENTS = new HashMap<String, SoundEventAccessorComposite>();
    private static final Set<Object> PLAYING = Collections.newSetFromMap(new WeakHashMap<Object, Boolean>());
    private static final Map<Object, Record> RECORDS = Collections.synchronizedMap(new WeakHashMap<Object, Record>());
    private static final Map<Object, ResourceLocation> EVENT_LOCATION = new WeakHashMap<Object, ResourceLocation>();
    private static final Map<Object, SoundCategory> EVENT_CATEGORY = new WeakHashMap<Object, SoundCategory>();
    private static boolean paused;
    private static Class<?> ATTENUATION;

    private Sounds() {}

    /** Fields of one {@code PositionedSound}; its stubbed constructor never assigns them. */
    private static final class Record {
        ResourceLocation location;
        float volume = 1.0F;
        float pitch = 1.0F;
        float x;
        float y;
        float z;
        boolean repeat;
        int delay;
        int attenuation;
    }

    /** `Minecraft.getSoundHandler`: wire the handler's manager and registry by hand (all fields are final). */
    static void init(SoundHandler handler) throws Exception {
        if (get(handler, "field_147694_f") != null) {
            return;
        }
        SoundManager manager = allocate(SoundManager.class);
        set(manager, "field_148623_a", handler);
        set(manager, "field_148621_b", settings());
        set(handler, "field_147694_f", manager);
        // Vanilla's static `missing_sound` is only built by the stubbed SoundHandler.<clinit>.
        SoundPoolEntry missing = allocate(SoundPoolEntry.class);
        setStatic(SoundHandler.class, "field_147696_d", missing);
    }

    /** `ISound.getSoundLocation` as `domain:path`, the key of the event in sounds.json. */
    static String location(ISound sound) {
        ResourceLocation location = sound.func_147650_b();
        return location == null ? "" : location.toString();
    }

    /** The numbers `SoundRequest` keeps, read through the getters so mod-made `ISound`s work too. */
    static double[] params(ISound sound) throws Exception {
        Object attenuation = sound.func_147656_j();
        return new double[] {sound.func_147653_e(), sound.func_147655_f(), sound.func_147649_g(),
            sound.func_147654_h(), sound.func_147651_i(),
            attenuation == null ? 0 : (Integer) get(attenuation, "field_148589_c"),
            sound.func_147657_c() ? 1 : 0};
    }

    static void mark(Object sound) {
        PLAYING.add(sound);
    }

    static void stop(Object sound) {
        PLAYING.remove(sound);
    }

    static boolean playing(Object sound) {
        return PLAYING.contains(sound);
    }

    /** Keys Rust routes here: everything that only moves Java-visible sound state. */
    static Object invoke(String key, Object self, Object[] args) throws Exception {
        switch (key.substring(key.lastIndexOf('.') + 1)) {
            // PositionedSoundRecord factories (volume, attenuation from vanilla 1.8.9).
            case "func_147673_a":
                return create((ResourceLocation) args[0], 1.0F, 1.0F, 0.0F, 0.0F, 0.0F, 0);
            case "func_147674_a":
                return create((ResourceLocation) args[0], 0.25F, f(args[1]), 0.0F, 0.0F, 0.0F, 0);
            case "func_147675_a":
                return create((ResourceLocation) args[0], 4.0F, 1.0F, f(args[1]), f(args[2]), f(args[3]), 2);
            // ISound getters, declared in PositionedSound.
            case "func_147650_b":
                return record(self).location;
            case "func_147657_c":
                return record(self).repeat;
            case "func_147652_d":
                return record(self).delay;
            case "func_147653_e":
                return record(self).volume;
            case "func_147655_f":
                return record(self).pitch;
            case "func_147649_g":
                return record(self).x;
            case "func_147654_h":
                return record(self).y;
            case "func_147651_i":
                return record(self).z;
            case "func_147656_j":
                return attenuation(record(self).attenuation);
            // SoundEventAccessorComposite.
            case "func_148729_c":
                return EVENT_LOCATION.get(self);
            case "func_148728_d":
                return EVENT_CATEGORY.get(self);
            case "func_148721_a":
                return 1;
            case "func_148720_g":
                return missing(self);
            case "func_148727_a":
                return null;
            // SoundHandler.
            case "func_147680_a":
                return event((ResourceLocation) args[0]);
            case "func_147683_b":
                PLAYING.remove(args[0]);
                return null;
            case "func_147692_c":
                return PLAYING.contains(args[0]) && !paused;
            case "func_147689_b":
                paused = true;
                return null;
            case "func_147687_e":
                paused = false;
                return null;
            case "func_147690_c":
            case "func_148605_d":
                PLAYING.clear();
                return null;
            case "func_147685_d":
            case "func_148596_a":
                PLAYING.clear();
                paused = false;
                return null;
            case "func_147686_a":
                return null;
            // Sound levels live in GameSettings; the mixer keeps its own volumes.
            case "func_147684_a":
            case "func_148601_a":
            case "func_147691_a":
            case "func_148615_a":
            case "func_73660_a":
            case "func_148610_e":
            case "func_148613_b":
            case "func_110549_a":
            case "func_175085_a":
                return null;
            case "func_148595_a":
                return 1.0F;
            case "func_148597_a":
                return PLAYING.contains(args[0]) && !paused;
            default:
                return null;
        }
    }

    private static PositionedSoundRecord create(ResourceLocation location, float volume, float pitch, float x,
            float y, float z, int attenuation) {
        PositionedSoundRecord sound = allocate(PositionedSoundRecord.class);
        Record r = record(sound);
        r.location = location;
        r.volume = volume;
        r.pitch = pitch;
        r.x = x;
        r.y = y;
        r.z = z;
        r.attenuation = attenuation;
        return sound;
    }

    private static Record record(Object sound) {
        Record r = RECORDS.get(sound);
        if (r == null) {
            r = new Record();
            RECORDS.put(sound, r);
        }
        return r;
    }

    /** `SoundHandler.getSound`: every event is known here, so mods never see a null composite. */
    private static SoundEventAccessorComposite event(ResourceLocation location) {
        String key = location == null ? "" : location.toString();
        SoundEventAccessorComposite composite = EVENTS.get(key);
        if (composite == null) {
            composite = allocate(SoundEventAccessorComposite.class);
            EVENTS.put(key, composite);
            EVENT_LOCATION.put(composite, location);
            EVENT_CATEGORY.put(composite, SoundCategory.MASTER);
        }
        return composite;
    }

    private static SoundPoolEntry missing(Object composite) {
        return EVENT_LOCATION.containsKey(composite) ? allocate(SoundPoolEntry.class) : null;
    }

    private static GameSettings settings() {
        try {
            Minecraft mc = Minecraft.func_71410_x();
            return mc == null ? null : (GameSettings) get(mc, "field_71474_y");
        } catch (Throwable t) {
            return null;
        }
    }

    /** `ISound.AttenuationType` for a `getType()` value; the stub jar hides the nested enum from source. */
    private static Object attenuation(int type) throws Exception {
        if (ATTENUATION == null) {
            ATTENUATION = Class.forName("net.minecraft.client.audio.ISound$AttenuationType");
        }
        for (Object constant : ATTENUATION.getEnumConstants()) {
            if ((Integer) get(constant, "field_148589_c") == type) {
                return constant;
            }
        }
        return null;
    }

    private static float f(Object boxed) {
        return boxed instanceof Number ? ((Number) boxed).floatValue() : 0.0F;
    }

    private static Field field(Object target, String name) throws NoSuchFieldException {
        for (Class<?> c = target.getClass(); c != null; c = c.getSuperclass()) {
            try {
                Field f = c.getDeclaredField(name);
                f.setAccessible(true);
                return f;
            } catch (NoSuchFieldException ignored) {
                // look in the superclass: PositionedSoundRecord's fields are declared in PositionedSound
            }
        }
        throw new NoSuchFieldException(name);
    }

    private static Object get(Object target, String name) throws Exception {
        return target == null ? null : field(target, name).get(target);
    }

    private static void set(Object target, String name, Object value) throws Exception {
        if (target == null) {
            return;
        }
        try {
            field(target, name).set(target, value);
        } catch (ReflectiveOperationException | RuntimeException ignored) {
            // a field missing from the stub jar is not worth breaking sound init over
        }
    }

    private static void setStatic(Class<?> type, String name, Object value) {
        try {
            Field f = type.getDeclaredField(name);
            f.setAccessible(true);
            f.set(null, value);
        } catch (ReflectiveOperationException | RuntimeException ignored) {
        }
    }

    @SuppressWarnings("unchecked")
    private static <T> T allocate(Class<T> type) {
        return (T) Stub.call("rustmc.allocate", null, new Object[] {type});
    }
}
