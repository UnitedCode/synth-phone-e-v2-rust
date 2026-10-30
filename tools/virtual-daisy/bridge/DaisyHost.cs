// Direct Renode API host: deliberately never constructs its Python-backed Monitor.
using System;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Loader;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Extensions;
using Antmicro.Renode.Peripherals.CPU;
using Antmicro.Renode.PlatformDescription.UserInterface;
using Antmicro.Renode.Utilities;
using Antmicro.Renode.Time;
using Antmicro.Renode.PlatformDescription;
using Antmicro.Renode.PlatformDescription.Syntax;
using System.Collections.Generic;
using Antmicro.Renode.Peripherals.Miscellaneous;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals;
using System.Runtime.CompilerServices;
using Antmicro.Renode.Peripherals.Bus;

public static class DaisyHost
{
    public static void Main(string[] args)
    {
        string root = args[0];
        AssemblyLoadContext.Default.Resolving += (context, name) => {
            string path = name.Name == "Infrastructure" && Environment.GetEnvironmentVariable("VIRTUAL_DAISY_INFRASTRUCTURE") != null
                ? Environment.GetEnvironmentVariable("VIRTUAL_DAISY_INFRASTRUCTURE") : Path.Combine(root, name.Name + ".dll");
            return File.Exists(path) ? context.LoadFromAssemblyPath(path) : null;
        };
        if(args[1] == "--disable-python") { DisablePython(root, args[2]); return; }
        Boot(args);
    }
    private static void DisablePython(string root, string output)
    {
        var resolver = new Mono.Cecil.DefaultAssemblyResolver();
        resolver.AddSearchDirectory(root);
        using(var assembly = Mono.Cecil.AssemblyDefinition.ReadAssembly(Path.Combine(root, "Infrastructure.dll"), new Mono.Cecil.ReaderParameters { AssemblyResolver = resolver }))
        {
            var type = assembly.MainModule.Types.Single(t => t.FullName == "Antmicro.Renode.Core.PythonEngine");
            var constructor = type.Methods.Single(m => m.Name == ".cctor");
            constructor.Body.Instructions.Clear();
            constructor.Body.ExceptionHandlers.Clear();
            constructor.Body.Variables.Clear();
            var il = constructor.Body.GetILProcessor();
            il.Append(il.Create(Mono.Cecil.Cil.OpCodes.Ldstr, "Python execution is disabled in virtual Daisy"));
            il.Append(il.Create(Mono.Cecil.Cil.OpCodes.Newobj, assembly.MainModule.ImportReference(typeof(InvalidOperationException).GetConstructor(new[] {typeof(string)}))));
            il.Append(il.Create(Mono.Cecil.Cil.OpCodes.Throw));
            assembly.Write(output + ".tmp");
            File.Move(output + ".tmp", output, true);
        }
        Console.WriteLine("Created Python-disabled emulator assembly: " + output);
    }
    private static void Boot(string[] args)
    {
        Environment.CurrentDirectory = args[0];
        ConfigurationManager.Initialize(args[3] + ".config");
        string ignored;
        Misc.TryGetRootDirectory(args[0], out ignored);
        TypeManager.Instance.ScanFile(Assembly.GetExecutingAssembly().Location, false);
        VerifyPythonGuard();
        var emulation = EmulationManager.Instance.CurrentEmulation;
        var machine = new Machine();
        emulation.AddMachine(machine, "daisy");
        Logger.AddBackend(new FileBackend(args[3] + ".log", true), "virtual-daisy", true);
        var resolver = new PlatformDescriptionMachineExtensions.UsingResolver(new[] {args[0], Path.GetDirectoryName(args[1])});
        new CreationDriver(machine, resolver, new NoScripts()).ProcessFile(args[1]);
        var bus = machine.SystemBus;
        bus.LoadELF(args[2]);
        foreach(var entry in new[] { Tuple.Create("gpioPortA", new[] {0, 3, 7}), Tuple.Create("gpioPortB", new[] {1}), Tuple.Create("gpioPortC", new[] {0, 7, 9, 10, 11}) })
            foreach(int pin in entry.Item2) ((IGPIOReceiver)machine["sysbus." + entry.Item1]).OnGPIO(pin, true);
        ((DaisyGpio)machine["sysbus.gpioPortD"]).ConfigureStraps(uint.Parse(args[7]));
        Console.WriteLine("Board: " + args[6]);
        var cpu = (CortexM)machine["sysbus.cpu"];
        if(((DaisyCortexM)cpu).FetchTagVersion!=1) throw new InvalidOperationException("Missing native instruction-fetch tag");
        Console.WriteLine("Native instruction-fetch tag verified");
        cpu.VectorTableOffset = 0x08000000;
        cpu.SP = bus.ReadDoubleWord(0x08000000);
        cpu.PC = bus.ReadDoubleWord(0x08000004);
        cpu.PerformanceInMips = 480;
        emulation.MasterTimeSource.Quantum = TimeInterval.FromMicroseconds(1);
        if(args[5]=="architecture") { ArchitectureProbes(emulation,machine,args[3]); emulation.Dispose(); return; }
        var sai = (DaisySai)machine["sysbus.sai1"];
        byte[] bytes = File.ReadAllBytes(args[4]);
        if(bytes.Length % 8 != 0 || !BitConverter.IsLittleEndian) throw new InvalidOperationException("Expected little-endian stereo f32 input");
        var input = new float[bytes.Length / 4];
        Buffer.BlockCopy(bytes, 0, input, 0, bytes.Length);
        sai.LoadInput(input);
        sai.IdealLineInput = args[8] == "ideal-line";
        sai.SuppressReceive = args[5] == "drop-rx";
        using(var log = new StreamWriter(args[3] + ".rtt"))
        {
            IEnumerable<ulong> symbols;
            if(!((SystemBus)machine["sysbus"]).TryGetAllSymbolAddresses("_SEGGER_RTT", out symbols)) throw new InvalidOperationException("Firmware has no RTT symbol");
            ulong rtt = symbols.Single();
            for(int step = 0; step < 100; step++)
            {
                emulation.RunFor(TimeInterval.FromSeconds(0.01));
                if(step == 20 && args[5] == "cacheable-dma") {
                    uint selected = bus.ReadDoubleWord(0xE000ED98);
                    bus.WriteDoubleWord(0xE000ED98, 0);
                    uint rasr = bus.ReadDoubleWord(0xE000EDA0);
                    bus.WriteDoubleWord(0xE000EDA0, (rasr & ~0x003F0000U) | 0x000B0000U);
                    bus.WriteDoubleWord(0xE000ED98, selected);
                }
                if(step == 20 && args[5] == "dtcm-dma") {
                    uint cr=bus.ReadDoubleWord(0x40020010);
                    bus.WriteDoubleWord(0x40020010,cr&~1U);
                    bus.WriteDoubleWord(0x4002001c,0x20000000);
                    bus.WriteDoubleWord(0x40020020,0x20000000);
                    bus.WriteDoubleWord(0x40020010,cr|1U);
                }
                uint buffer = bus.ReadDoubleWord(rtt + 28);
                uint capacity = bus.ReadDoubleWord(rtt + 32);
                uint write = bus.ReadDoubleWord(rtt + 36);
                uint read = bus.ReadDoubleWord(rtt + 40);
                if(capacity == 0 || capacity > 65536 || read >= capacity || write >= capacity) continue;
                while(read != write) { log.Write((char)bus.ReadByte(buffer + read)); read = (read + 1) % capacity; }
                bus.WriteDoubleWord(rtt + 40, read);
            }
        }
        Console.WriteLine("PC: " + cpu.PC);
        File.WriteAllBytes(args[3], bus.ReadBytes(0x20000000UL, 0x20000));
        sai.Save(args[3] + ".wav");
        ((DaisyFmc)machine["sysbus.fmc"]).Save(args[3]+".sdram.txt");
        ((DaisyNvic)machine["sysbus.nvic"]).SaveMemory(args[3] + ".memory.txt");
        File.WriteAllText(args[3]+".bus.txt",$"dma1_lisr={bus.ReadDoubleWord(0x40020000)}\ndma1_s0cr={bus.ReadDoubleWord(0x40020010)}\ndma1_s0ndtr={bus.ReadDoubleWord(0x40020014)}\n");
        emulation.Dispose();
    }
    private static void ArchitectureProbes(Emulation emulation,Machine machine,string prefix)
    {
        var bus=machine.SystemBus;
        var cpu=(DaisyCortexM)machine["sysbus.cpu"];
        var architecture=((DaisyNvic)machine["sysbus.nvic"]).Architecture;
        bus.WriteDoubleWord(0x24040000,0x47702001); // movs r0,#1; bx lr.
        bus.WriteBytes(File.ReadAllBytes(Path.Combine(Path.GetDirectoryName(prefix),"architecture-probe.bin")),0x1000);
        bus.WriteDoubleWord(0xe000ed14,3U<<16);
        cpu.PC=0x1001;
        emulation.RunFor(TimeInterval.FromMilliseconds(1));
        architecture.Write(0x24040000,4,0x47702003,false); // DMA/physical path, no I-cache snooping.
        bus.WriteDoubleWord(0x2001e03cUL,1); // Release the ARM program's mailbox wait.
        emulation.RunFor(TimeInterval.FromMilliseconds(1));
        File.WriteAllBytes(prefix+".architecture.bin",bus.ReadBytes(0x2001e000UL,40));
        architecture.Save(prefix+".memory.txt");
        Console.WriteLine("ARM instruction-cache probe executed; results require Rust grading");
    }
    private static void VerifyPythonGuard()
    {
        var constructor = typeof(PythonEngine).TypeInitializer;
        byte[] il = constructor.GetMethodBody().GetILAsByteArray();
        if(il.Length != 11 || il[0] != 0x72 || il[5] != 0x73 || il[10] != 0x7a || constructor.Module.ResolveString(BitConverter.ToInt32(il, 1)) != "Python execution is disabled in virtual Daisy")
            throw new InvalidOperationException("Refusing to run without the Python-disabled emulator assembly");
        try { RuntimeHelpers.RunClassConstructor(typeof(PythonEngine).TypeHandle); }
        catch(TypeInitializationException error) when(error.InnerException is InvalidOperationException && error.InnerException.Message == "Python execution is disabled in virtual Daisy") { Console.WriteLine("Python execution guard verified"); return; }
        throw new InvalidOperationException("Python guard did not block initialization");
    }
    private sealed class NoScripts : IScriptHandler
    {
        public bool ValidateIsEntry(IScriptable scriptable, string sectionName, out string message) { message = "Executable platform sections are disabled"; return false; }
        public void Execute(IScriptable scriptable, IEnumerable<string> statements, Action<string> errorHandler) { throw new InvalidOperationException("Executable platform sections are disabled"); }
        public void RegisterReset(IScriptable scriptable, IEnumerable<string> statements, Action<string> errorHandler) { throw new InvalidOperationException("Executable platform sections are disabled"); }
        public IDisposable PushDirectory(string directory) { return new Scope(); }
        private sealed class Scope : IDisposable { public void Dispose() {} }
    }
}
