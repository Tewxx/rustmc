package rustmc.extract;

import com.google.gson.JsonPrimitive;

final class Json {
    private Json() {}

    static JsonPrimitive num(double d) {
        return new JsonPrimitive(d);
    }

    static JsonPrimitive str(String s) {
        return new JsonPrimitive(s);
    }
}
