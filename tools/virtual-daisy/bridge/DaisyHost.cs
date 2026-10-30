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
            // Dispatch the budget check through its existing virtual getter.
            // Only the initial condition is changed; the two subsequent field
            // reads stay atomic with the CPU thread's budget subtraction.
            var cpu=assembly.MainModule.Types.Single(t=>t.FullName=="Antmicro.Renode.Peripherals.CPU.BaseCPU");
            var body=cpu.Methods.Single(m=>m.Name=="CpuThreadBodyInner");
            var getter=cpu.Methods.Single(m=>m.Name=="get_SkipInstructions");
            var reads=body.Body.Instructions.Where(i=>i.OpCode==Mono.Cecil.Cil.OpCodes.Ldfld && i.Operand is Mono.Cecil.FieldReference f && f.Name=="skipInstructions").ToArray();
            if(reads.Length!=3) throw new InvalidOperationException("Pinned CPU pacing adapter point changed");
            reads[0].OpCode=Mono.Cecil.Cil.OpCodes.Callvirt;reads[0].Operand=getter;
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
        VerifyPacingAdapter();
        var emulation = EmulationManager.Instance.CurrentEmulation;
        var machine = new Machine();
        emulation.AddMachine(machine, "daisy");
        Logger.AddBackend(new FileBackend(args[3] + ".log", true), "virtual-daisy", true);
        var resolver = new PlatformDescriptionMachineExtensions.UsingResolver(new[] {args[0], Path.GetDirectoryName(args[1])});
        new CreationDriver(machine, resolver, new NoScripts()).ProcessFile(args[1]);
        var bus = machine.SystemBus;
        var imageArchitecture=((DaisyNvic)machine["sysbus.nvic"]).Architecture;
        imageArchitecture.Register(2,0,args[9]=="maximum" ? 1U:0U,true);
        imageArchitecture.ImageLoading=true;
        try { bus.LoadELF(args[2]); } finally { imageArchitecture.ImageLoading=false; }
        foreach(var entry in new[] { Tuple.Create("gpioPortA", new[] {0, 3, 7}), Tuple.Create("gpioPortB", new[] {1}), Tuple.Create("gpioPortC", new[] {0, 7, 9, 10, 11}) })
            foreach(int pin in entry.Item2) ((IGPIOReceiver)machine["sysbus." + entry.Item1]).OnGPIO(pin, true);
        ((DaisyGpio)machine["sysbus.gpioPortD"]).ConfigureStraps(uint.Parse(args[7]));
        Console.WriteLine("Board: " + args[6]);
        var cpu = (CortexM)machine["sysbus.cpu"];
        if(((DaisyCortexM)cpu).FetchTagVersion!=8) throw new InvalidOperationException("Missing native instruction-fetch/runtime adapter");
        Console.WriteLine("Native instruction-fetch tag verified");
        cpu.VectorTableOffset = 0x08000000;
        cpu.SP = bus.ReadDoubleWord(0x08000000);
        cpu.PC = bus.ReadDoubleWord(0x08000004);
        cpu.PerformanceInMips = 480;
        emulation.MasterTimeSource.Quantum = TimeInterval.FromMicroseconds(1);
        if(args[5]=="architecture" || args[5]=="runtime-fault") { ArchitectureProbes(emulation,machine,args[3],args[5]=="runtime-fault"); emulation.Dispose(); return; }
        if(args[5]=="flash" || args[5]=="flash-wait-fault") {FlashProbes(emulation,machine,args[3],args[5]=="flash-wait-fault");emulation.Dispose();return;}
        if(args[5]=="timing" || args[5]=="dsb-fault" || args[5]=="entry-fault") {TimingProbes(emulation,machine,args[3],args[5]);emulation.Dispose();return;}
        if(args[5]=="wait-irq" || args[5]=="wait-irq-fault" || args[5]=="wait-irq-target") {InterruptProbes(emulation,machine,args[3],args[5]);emulation.Dispose();return;}
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
                if(step%10==0) {Console.WriteLine($"Audio progress: {(step+1)*10} ms, PC={cpu.PC}");imageArchitecture.Save(args[3]+".progress.txt");}
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
                    emulation.RunFor(TimeInterval.FromMicroseconds(20));
                    if((bus.ReadDoubleWord(0x40020010)&1)!=0) throw new InvalidOperationException("DMA did not finish its active transfer before reconfiguration");
                    bus.WriteDoubleWord(0x4002001c,0x20000000);
                    bus.WriteDoubleWord(0x40020020,0x20000000);
                    bus.WriteDoubleWord(0x40020010,cr|1U);
                }
                if(step==20 && args[5]=="bus-contention") ((DaisyNvic)machine["sysbus.nvic"]).Architecture.InjectBusLoad(8,2000000);
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
        ((DaisyFlashController)machine["sysbus.flashController"]).Save(args[3]+".flash.txt");
        ((DaisyDma)machine["sysbus.dma1"]).Save(args[3]+".dma.txt");
        ((DaisyNvic)machine["sysbus.nvic"]).SaveMemory(args[3] + ".memory.txt");
        File.WriteAllText(args[3]+".bus.txt",$"dma1_lisr={bus.ReadDoubleWord(0x40020000)}\ndma1_s0cr={bus.ReadDoubleWord(0x40020010)}\ndma1_s0ndtr={bus.ReadDoubleWord(0x40020014)}\n");
        emulation.Dispose();
    }
    private static void ArchitectureProbes(Emulation emulation,Machine machine,string prefix,bool disableRuntime)
    {
        var bus=machine.SystemBus;
        var cpu=(DaisyCortexM)machine["sysbus.cpu"];
        var architecture=((DaisyNvic)machine["sysbus.nvic"]).Architecture;
        bus.WriteDoubleWord(0x24040000,0x47702001); // movs r0,#1; bx lr.
        architecture.LoadFlash(0x0801e000,4,0x47702009);
        bus.WriteDoubleWord(0x24042000,0x47702008);
        bus.WriteBytes(File.ReadAllBytes(Path.Combine(Path.GetDirectoryName(prefix),"architecture-probe.bin")),0x1000);
        bus.WriteDoubleWord(0xe000ed14,3U<<16);
        cpu.PC=0x1001;
        emulation.RunFor(TimeInterval.FromMilliseconds(1));
        architecture.Write(0x24040000,4,0x47702003,false); // DMA/physical path, no I-cache snooping.
        if(disableRuntime) cpu.DisableRuntimeFetching();
        bus.WriteDoubleWord(0x2001e03cUL,1); // Release the ARM program's mailbox wait.
        emulation.RunFor(TimeInterval.FromMilliseconds(1));
        File.WriteAllBytes(prefix+".architecture.bin",bus.ReadBytes(0x2001e000UL,60));
        architecture.Save(prefix+".memory.txt");
        Console.WriteLine("ARM instruction-cache probe executed; results require Rust grading");
    }
    private static void FlashProbes(Emulation emulation,Machine machine,string prefix,bool bypassWait) {
        var bus=machine.SystemBus;var cpu=(DaisyCortexM)machine["sysbus.cpu"];
        var architecture=((DaisyNvic)machine["sysbus.nvic"]).Architecture;
        architecture.LoadFlash(0x0801e000,4,0x47702009);
        bus.WriteBytes(File.ReadAllBytes(Path.Combine(Path.GetDirectoryName(prefix),"flash-probe.bin")),0x2000);
        bus.WriteDoubleWord(0xe000ed14,1U<<17);
        architecture.CpuWaitBypass=bypassWait;
        cpu.PC=0x2001;
        emulation.RunFor(TimeInterval.FromMilliseconds(Environment.GetEnvironmentVariable("VIRTUAL_DAISY_FLASH_MAXIMUM")=="1" ? 2003UL:1002UL));
        File.WriteAllBytes(prefix+".flash.bin",bus.ReadBytes(0x2001d000UL,40));
        architecture.Save(prefix+".memory.txt");
        Console.WriteLine("ARM flash programming probe executed; results require Rust grading");
    }
    private static void TimingProbes(Emulation emulation,Machine machine,string prefix,string fault) {
        var bus=machine.SystemBus;var cpu=(DaisyCortexM)machine["sysbus.cpu"];
        var architecture=((DaisyNvic)machine["sysbus.nvic"]).Architecture;
        architecture.LoadFlash(0x0801e02c,4,0x3801);
        bus.WriteBytes(File.ReadAllBytes(Path.Combine(Path.GetDirectoryName(prefix),"timing-probe.bin")),0x3000);
        bus.WriteDoubleWord(0xe000ed98,0);
        bus.WriteDoubleWord(0xe000ed9c,0x3001e000);
        bus.WriteDoubleWord(0xe000eda0,0x030c0019);
        bus.WriteDoubleWord(0xe000ed94,5);
        bus.WriteDoubleWord(0xe000ed14,3U<<16);
        bus.WriteDoubleWord(0xe000edfc,1U<<24);
        bus.WriteDoubleWord(0xe0001fb0,0xc5acce55);
        bus.WriteDoubleWord(0xe0001000,1);
        bus.WriteDoubleWord(0xe0001004,0);
        cpu.VectorTableOffset=0x0801e000;
        cpu.SP=0x3001f000;cpu.PC=0x3001;
        architecture.InjectBusLoad(7,100000,true);
        if(fault=="dsb-fault") cpu.DisableBarriers();
        if(fault=="entry-fault") architecture.Register(3,0,1,true);
        emulation.RunFor(TimeInterval.FromMicroseconds(50));
        File.WriteAllBytes(prefix+".timing-early.bin",bus.ReadBytes(0x2001c000UL,4));
        for(int step=0;step<45;step++) {
            emulation.RunFor(TimeInterval.FromMicroseconds(10));
            if(step%5==0) {Console.WriteLine($"Timing progress: {60+step*10} us, PC={cpu.PC}, SP={cpu.SP}");architecture.Save(prefix+".timing-progress.txt");File.WriteAllBytes(prefix+".timing-progress.bin",bus.ReadBytes(0x2001c000UL,48));}
        }
        File.WriteAllBytes(prefix+".timing.bin",bus.ReadBytes(0x2001c000UL,48));
        architecture.Save(prefix+".memory.txt");
        Console.WriteLine("ARM writeback/exception timing probe executed; results require Rust grading");
    }
    private static void InterruptProbes(Emulation emulation,Machine machine,string prefix,string fault) {
        var bus=machine.SystemBus;var cpu=(DaisyCortexM)machine["sysbus.cpu"];
        bool otherTarget=fault=="wait-irq-target";
        bus.WriteBytes(File.ReadAllBytes(Path.Combine(Path.GetDirectoryName(prefix),otherTarget?"interrupt-target-probe.bin":"interrupt-probe.bin")),0x5000);
        bus.WriteDoubleWord(0x6040,0x5801);cpu.VectorTableOffset=0x6000;
        bus.WriteDoubleWord(0xe000edfc,1U<<24);bus.WriteDoubleWord(0xe0001fb0,0xc5acce55);
        bus.WriteDoubleWord(0xe0001000,1);bus.WriteDoubleWord(0xe0001004,0);
        cpu.SP=0x2001f000;cpu.PC=0x5001;
        if(otherTarget) {
            cpu.SP=0x3001f000;bus.WriteDoubleWord(0x24040000,42);
            ((DaisyNvic)machine["sysbus.nvic"]).Architecture.InjectBusLoad(7,100000);
        }
        cpu.DisableWaitPreemption=fault=="wait-irq-fault";
        emulation.RunFor(TimeInterval.FromMicroseconds(20));
        bus.WriteDoubleWord(0xe000e100,1);bus.WriteDoubleWord(0xe000e200,1);
        emulation.RunFor(TimeInterval.FromMicroseconds(10));
        File.WriteAllBytes(prefix+".interrupt-early.bin",bus.ReadBytes(0x2001b008UL,4));
        emulation.RunFor(TimeInterval.FromMicroseconds(470));
        File.WriteAllBytes(prefix+".interrupt.bin",bus.ReadBytes(0x2001b000UL,20));
        ((DaisyNvic)machine["sysbus.nvic"]).Architecture.Save(prefix+".memory.txt");
        Console.WriteLine("ARM interrupt-during-memory-wait probe executed; results require Rust grading");
    }
    private static void VerifyPacingAdapter() {
        using(var assembly=Mono.Cecil.AssemblyDefinition.ReadAssembly(typeof(BaseCPU).Assembly.Location)) {
            var cpu=assembly.MainModule.Types.Single(t=>t.FullName=="Antmicro.Renode.Peripherals.CPU.BaseCPU");
            var method=cpu.Methods.Single(m=>m.Name=="CpuThreadBodyInner");
            int reads=method.Body.Instructions.Count(i=>i.OpCode==Mono.Cecil.Cil.OpCodes.Ldfld && i.Operand is Mono.Cecil.FieldReference f && f.Name=="skipInstructions");
            int calls=method.Body.Instructions.Count(i=>i.OpCode==Mono.Cecil.Cil.OpCodes.Callvirt && i.Operand is Mono.Cecil.MethodReference m && m.Name=="get_SkipInstructions" && m.DeclaringType.FullName==cpu.FullName);
            if(reads!=2 || calls!=1) throw new InvalidOperationException("Missing native-wait IRQ pacing adapter");
        }
        Console.WriteLine("IRQ-aware wait pacing adapter verified");
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
