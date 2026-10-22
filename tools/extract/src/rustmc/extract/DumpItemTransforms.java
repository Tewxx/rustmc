package rustmc.extract;
import com.google.gson.*;
import java.io.*;
import java.net.JarURLConnection;
import java.util.*;
import java.util.jar.*;
import net.minecraft.client.renderer.block.model.*;
import net.minecraft.util.ResourceLocation;
import org.lwjgl.util.vector.Vector3f;

/** Uses ModelBlock's real deserializer and parent-transform resolution; no GL context. */
public final class DumpItemTransforms {
    public static JsonArray dump() throws Exception {
        JarFile jar=((JarURLConnection)DumpItemTransforms.class.getResource("/assets/minecraft/models/item/stone.json").openConnection()).getJarFile();
        Map<ResourceLocation,ModelBlock> models=new TreeMap<ResourceLocation,ModelBlock>(new Comparator<ResourceLocation>() {
            public int compare(ResourceLocation a,ResourceLocation b){return a.toString().compareTo(b.toString());}
        });
        Enumeration<JarEntry> entries=jar.entries();
        while(entries.hasMoreElements()) {
            String p=entries.nextElement().getName();String prefix="assets/minecraft/models/";
            if(!p.startsWith(prefix)||!p.endsWith(".json"))continue;
            Reader r=new InputStreamReader(jar.getInputStream(jar.getJarEntry(p)),"UTF-8");
            try {models.put(new ResourceLocation(p.substring(prefix.length(),p.length()-5)),ModelBlock.deserialize(r));} finally {r.close();}
        }
        for(ModelBlock model:models.values())model.getParentFromMap(models);
        JsonArray out=new JsonArray();
        for(Map.Entry<ResourceLocation,ModelBlock> e:models.entrySet()) {
            JsonObject o=new JsonObject();o.addProperty("model",e.getKey().getResourcePath());
            ItemCameraTransforms transforms=e.getValue().getAllTransforms();
            String[] names={"thirdperson","firstperson","head","gui","ground","fixed"};
            ItemCameraTransforms.TransformType[] types={ItemCameraTransforms.TransformType.THIRD_PERSON,ItemCameraTransforms.TransformType.FIRST_PERSON,ItemCameraTransforms.TransformType.HEAD,ItemCameraTransforms.TransformType.GUI,ItemCameraTransforms.TransformType.GROUND,ItemCameraTransforms.TransformType.FIXED};
            JsonObject display=new JsonObject();
            for(int i=0;i<names.length;i++) {ItemTransformVec3f t=transforms.getTransform(types[i]);JsonObject v=new JsonObject();v.add("rotation",vector(t.rotation));v.add("translation",vector(t.translation));v.add("scale",vector(t.scale));display.add(names[i],v);}
            o.add("display",display);out.add(o);
        }
        return out;
    }
    private static JsonArray vector(Vector3f v){JsonArray a=new JsonArray();a.add(new JsonPrimitive(v.x));a.add(new JsonPrimitive(v.y));a.add(new JsonPrimitive(v.z));return a;}
}
