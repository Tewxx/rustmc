package ft;

import java.io.*;
import java.util.*;

/**
 * Deterministic trace writer shared by every test mod. One JSON line per call:
 * {"seq":N,"test":"EV-01","event":"name", ...key/values in call order}.
 * Output file: ${ft.trace.dir}/${group}.jsonl (default dir "ft-trace"); truncated on first use per JVM.
 * Values: null/Boolean/Number/String; arrays and Iterables become JSON arrays; anything else is toString()'d.
 */
public final class Trace {
    private static final Map<String, PrintWriter> writers = new HashMap<String, PrintWriter>();
    private static final Map<String, Integer> seqs = new HashMap<String, Integer>();

    private Trace() {}

    public static synchronized void log(String group, String test, String event, Object... kv) {
        try {
            PrintWriter w = writers.get(group);
            if (w == null) {
                File dir = new File(System.getProperty("ft.trace.dir", "ft-trace"));
                dir.mkdirs();
                w = new PrintWriter(new OutputStreamWriter(new FileOutputStream(new File(dir, group + ".jsonl")), "UTF-8"));
                writers.put(group, w);
                seqs.put(group, 0);
            }
            int seq = seqs.get(group) + 1;
            seqs.put(group, seq);
            StringBuilder sb = new StringBuilder();
            sb.append("{\"seq\":").append(seq).append(",\"test\":");
            str(sb, test);
            sb.append(",\"event\":");
            str(sb, event);
            for (int i = 0; i + 1 < kv.length; i += 2) {
                sb.append(',');
                str(sb, String.valueOf(kv[i]));
                sb.append(':');
                val(sb, kv[i + 1]);
            }
            sb.append('}');
            w.println(sb);
            w.flush();
        } catch (IOException e) {
            throw new RuntimeException(e);
        }
    }

    private static void val(StringBuilder sb, Object v) {
        if (v == null) sb.append("null");
        else if (v instanceof Boolean || v instanceof Integer || v instanceof Long || v instanceof Short || v instanceof Byte) sb.append(v);
        else if (v instanceof Float || v instanceof Double) {
            double d = ((Number) v).doubleValue();
            if (Double.isNaN(d) || Double.isInfinite(d)) str(sb, String.valueOf(d));
            else sb.append(d == Math.rint(d) && Math.abs(d) < 1e12 ? String.valueOf((long) d) : String.format(Locale.ROOT, "%.4f", d));
        } else if (v instanceof Object[]) {
            val(sb, Arrays.asList((Object[]) v));
        } else if (v instanceof Iterable) {
            sb.append('[');
            boolean first = true;
            for (Object o : (Iterable<?>) v) {
                if (!first) sb.append(',');
                first = false;
                val(sb, o);
            }
            sb.append(']');
        } else str(sb, String.valueOf(v));
    }

    private static void str(StringBuilder sb, String s) {
        sb.append('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"': sb.append("\\\""); break;
                case '\\': sb.append("\\\\"); break;
                case '\n': sb.append("\\n"); break;
                case '\r': sb.append("\\r"); break;
                case '\t': sb.append("\\t"); break;
                default:
                    if (c < 0x20 || c > 0x7e) sb.append(String.format("\\u%04x", (int) c));
                    else sb.append(c);
            }
        }
        sb.append('"');
    }

    /** Per-group convenience handle. */
    public static final class T {
        public final String group;
        public T(String group) { this.group = group; }
        public void log(String test, String event, Object... kv) { Trace.log(group, test, event, kv); }
    }

    public static T of(String group) { return new T(group); }
}
