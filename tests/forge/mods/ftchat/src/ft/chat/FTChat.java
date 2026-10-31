package ft.chat;

import ft.ScriptEvent;
import ft.Trace;
import java.util.*;
import net.minecraft.client.Minecraft;
import net.minecraft.command.CommandBase;
import net.minecraft.command.CommandException;
import net.minecraft.command.ICommandSender;
import net.minecraft.util.BlockPos;
import net.minecraft.util.ChatComponentText;
import net.minecraftforge.client.ClientCommandHandler;
import net.minecraftforge.client.event.ClientChatReceivedEvent;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.FMLInitializationEvent;
import net.minecraftforge.fml.common.event.FMLPreInitializationEvent;
import net.minecraftforge.fml.common.eventhandler.SubscribeEvent;

/** CHAT-*: ClientChatReceivedEvent (observe / cancel / replace), sending chat, ClientCommandHandler commands. */
@Mod(modid = "ftchat", name = "FT Chat", version = "1.0", dependencies = "required-after:ftcommon",
     acceptableRemoteVersions = "*", clientSideOnly = true)
public class FTChat {
    static final Trace.T T = Trace.of("chat");

    public static class FtCmd extends CommandBase {
        @Override public String getCommandName() { return "ftcmd"; }
        @Override public String getCommandUsage(ICommandSender sender) { return "/ftcmd <args>"; }
        @Override public int getRequiredPermissionLevel() { return 0; }
        @Override public void processCommand(ICommandSender sender, String[] args) throws CommandException {
            T.log("CHAT-04", "ClientCommand.process", "args", Arrays.asList(args), "sender", sender.getName(),
                "senderClass", sender.getClass().getSimpleName());
            sender.addChatMessage(new ChatComponentText("ftcmd ran with " + args.length + " args"));
        }
        @Override public List<String> addTabCompletionOptions(ICommandSender sender, String[] args, BlockPos pos) {
            return getListOfStringsMatchingLastWord(args, "alpha", "beta", "gamma");
        }
    }

    @Mod.EventHandler
    public void preInit(FMLPreInitializationEvent e) {
        MinecraftForge.EVENT_BUS.register(this);
    }

    @Mod.EventHandler
    public void init(FMLInitializationEvent e) {
        ClientCommandHandler.instance.registerCommand(new FtCmd());
        T.log("CHAT-04", "registered", "commands", new TreeSet<String>(ClientCommandHandler.instance.getCommands().keySet()));
    }

    @SubscribeEvent
    public void chat(ClientChatReceivedEvent e) {
        String text = e.message.getUnformattedText();
        String action = "pass";
        if (text.contains("[cancel-me]")) { e.setCanceled(true); action = "cancelled"; }
        else if (text.contains("[replace-me]")) { e.message = new ChatComponentText("replaced-by-ftchat"); action = "replaced"; }
        T.log("CHAT-01", "ClientChatReceivedEvent", "type", (int) e.type, "text", text, "formatted", e.message.getFormattedText().replace('§', '&'),
            "action", action, "cancelable", e.isCancelable());
    }

    @SubscribeEvent
    public void script(ScriptEvent e) {
        Minecraft mc = Minecraft.getMinecraft();
        if (e.step.equals("chat")) {
            mc.thePlayer.sendChatMessage("hello ft");               // goes to the server, echoed back as "<FTBot> hello ft"
            mc.thePlayer.sendChatMessage("/ftcmd one two");         // consumed client-side, never sent
            mc.thePlayer.sendChatMessage("/ftunknown");             // not client-handled: sent, server answers "Unknown command"
            T.log("CHAT-04", "tab-complete", "options",
                ClientCommandHandler.instance.getTabCompletionOptions(mc.thePlayer, "ftcmd ", mc.thePlayer.getPosition()));
        } else if (e.step.equals("end")) {
            T.log("CHAT-02", "sent-history", "messages", mc.ingameGUI.getChatGUI().getSentMessages());
        }
    }
}
