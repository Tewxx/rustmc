package ft;

import net.minecraftforge.fml.common.eventhandler.Event;

/** Posted on MinecraftForge.EVENT_BUS by the FTCommon driver; test mods subscribe to run their scripted step. */
public class ScriptEvent extends Event {
    /** Step names, in order: joined, gui_open, gui_interact, gui_close, keypress, chat, net, interact, entity, end. */
    public final String step;
    /** Client ticks since the player entered the world. */
    public final int tick;

    public ScriptEvent(String step, int tick) {
        this.step = step;
        this.tick = tick;
    }
}
