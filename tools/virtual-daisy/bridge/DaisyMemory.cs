// Required Renode interfaces; RAM, MPU cache policy and cache maintenance are Rust.
using System;
using System.Runtime.InteropServices;
using System.Collections.Generic;
using Antmicro.Renode.Core;
using Antmicro.Renode.Peripherals;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.IRQControllers;
using Antmicro.Renode.Time;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public sealed class DaisyArchitecture : IPeripheral
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong ReadMemory(IntPtr state, uint address, uint width, uint cpu);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint WriteMemory(IntPtr state, uint address, uint width, uint value, uint cpu);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint ContainsMemory(uint address, uint width);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint SaveNative(IntPtr state, [MarshalAs(UnmanagedType.LPUTF8Str)] string path);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint SyncNative(IntPtr state,IntPtr clock,IntPtr fmc,ulong now);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong AccessNative(IntPtr state,ulong context,uint address,uint width,ref ulong value,uint write);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong DebtNative(IntPtr state);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong RegNative(IntPtr state,uint space,ulong offset,uint value,uint write,uint width);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong PortNative(IntPtr state,uint address,uint write,ulong token);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint LoadNative(IntPtr state,uint target,ulong ns,uint write);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint TimeNative(IntPtr state,ulong now);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint IrqNative(IntPtr state);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void AbandonNative(IntPtr state,ulong ns);
        public DaisyArchitecture(IMachine machine,string libraryPath)
        {
            this.machine=machine;
            native = new DaisyNative(libraryPath, 12, 0x1000);
            var library = NativeLibrary.Load(libraryPath);
            read = Marshal.GetDelegateForFunctionPointer<ReadMemory>(NativeLibrary.GetExport(library, "daisy_memory_read"));
            write = Marshal.GetDelegateForFunctionPointer<WriteMemory>(NativeLibrary.GetExport(library, "daisy_memory_write"));
            contains = Marshal.GetDelegateForFunctionPointer<ContainsMemory>(NativeLibrary.GetExport(library, "daisy_memory_is_ram"));
            save = Marshal.GetDelegateForFunctionPointer<SaveNative>(NativeLibrary.GetExport(library, "daisy_memory_save"));
            RuntimeFetchPointer=NativeLibrary.GetExport(library,"daisy_runtime_fetch");
            BarrierPointer=NativeLibrary.GetExport(library,"daisy_barrier");
            ExceptionPointer=NativeLibrary.GetExport(library,"daisy_exception");
            synchronize=Marshal.GetDelegateForFunctionPointer<SyncNative>(NativeLibrary.GetExport(library,"daisy_bus_sync"));
            access=Marshal.GetDelegateForFunctionPointer<AccessNative>(NativeLibrary.GetExport(library,"daisy_cpu_access"));
            debt=Marshal.GetDelegateForFunctionPointer<DebtNative>(NativeLibrary.GetExport(library,"daisy_cpu_debt"));
            register=Marshal.GetDelegateForFunctionPointer<RegNative>(NativeLibrary.GetExport(library,"daisy_shared_reg"));
            port=Marshal.GetDelegateForFunctionPointer<PortNative>(NativeLibrary.GetExport(library,"daisy_dma_port"));
            load=Marshal.GetDelegateForFunctionPointer<LoadNative>(NativeLibrary.GetExport(library,"daisy_bus_load"));
            advance=Marshal.GetDelegateForFunctionPointer<TimeNative>(NativeLibrary.GetExport(library,"daisy_time_sync"));
            flashIrq=Marshal.GetDelegateForFunctionPointer<IrqNative>(NativeLibrary.GetExport(library,"daisy_flash_irq"));
            abandon=Marshal.GetDelegateForFunctionPointer<AbandonNative>(NativeLibrary.GetExport(library,"daisy_abandon_wait"));
            waitInterruptible=Marshal.GetDelegateForFunctionPointer<IrqNative>(NativeLibrary.GetExport(library,"daisy_wait_interruptible"));
        }
        public uint Read(uint address, uint width, bool cpu)
        {
            return ReadMode(address,width,cpu ? 1U:0U);
        }
        public uint ReadMode(uint address,uint width,uint mode) { if(mode==1) {ulong data=0;CpuTransfer(address,width,false,ref data);return (uint)data;} lock(sync) { if(mode==2) codePages.Add(address>>12); ulong value=read(native.State,address,width,mode); if(value>>32!=0) throw new InvalidOperationException($"Unsupported RAM access: 0x{address:X}"); return (uint)value; } }
        public void AttachCPU(DaisyCortexM cpu) { this.cpu=cpu; }
        public IntPtr NativeState => native.State;
        public IntPtr RuntimeFetchPointer { get; }
        public IntPtr BarrierPointer {get;}
        public IntPtr ExceptionPointer {get;}
        public bool ImageLoading { get; set; }
        public bool CpuWaitBypass { get; set; }
        public void Synchronize(bool reportCpu=false) {
            if(reportCpu && machine.SystemBus.TryGetCurrentCPU(out var current)) current.SyncTime();
            var rcc=(DaisyRcc)machine["sysbus.rcc"];
            var fmc=(DaisyFmc)machine["sysbus.fmc"];
            if(rcc.WithState(clock=>fmc.WithState(controller=>synchronize(native.State,clock,controller,machine.ElapsedVirtualTime.TimeElapsed.TotalNanoseconds)))!=0) throw new InvalidOperationException("Unsupported bus clock configuration");
        }
        public ulong TakeCpuDebt() { return debt(native.State); }
        public void AbandonWait(ulong ns) {abandon(native.State,ns);}
        public bool WaitInterruptible=>waitInterruptible(native.State)!=0;
        public void Advance() {if(advance(native.State,machine.ElapsedVirtualTime.TimeElapsed.TotalNanoseconds)!=0) throw new InvalidOperationException("Bus time synchronization failed");}
        public bool FlashIrq => flashIrq(native.State)!=0;
        public void CpuTransfer(uint address,uint width,bool isWrite,ref ulong value) {
            Synchronize(true);
            if(CpuWaitBypass) {
                if(isWrite) {if(write(native.State,address,Math.Min(width,4U),(uint)value,1)!=0 || (width==8 && write(native.State,address+4,4,(uint)(value>>32),1)!=0)) throw new InvalidOperationException("Bypass write failed");}
                else {ulong low=read(native.State,address,Math.Min(width,4U),1);if(low>>32!=0) throw new InvalidOperationException("Bypass read failed");value=(uint)low;if(width==8) {ulong high=read(native.State,address+4,4,1);if(high>>32!=0) throw new InvalidOperationException("Bypass read failed");value|=high<<32;}}
                return;
            }
            ulong ns=access(native.State,cpu.MemoryContext,address,width,ref value,isWrite?1U:0U);
            if(ns==ulong.MaxValue) throw new InvalidOperationException("Unsupported CPU bus access");
            if(ns!=0) {value=0;cpu.StallMemoryAccess();}
        }
        public uint Register(uint space,long offset,uint value,bool write,uint width=4) {
            Synchronize(true);
            ulong result=register(native.State,space,(ulong)offset,value,write?1U:0U,width);
            if(result>>32!=0) throw new InvalidOperationException($"Unsupported flash/AXI register access at 0x{offset:X}");
            return (uint)result;
        }
        public ulong DmaPort(uint address,bool write,ulong token) { ulong result=port(native.State,address,write?1U:0U,token==1 ? 0:token); if(result==ulong.MaxValue) throw new InvalidOperationException("Unmodeled DMA bus transfer"); return result; }
        public void InjectBusLoad(uint target,ulong ns,bool write=false) {Synchronize();if(load(native.State,target,ns,write ? 1U:0U)!=0) throw new InvalidOperationException("Could not inject bus load");}
        public void LoadFlash(uint address,uint width,uint value) { lock(sync) { if(write(native.State,address,width,value,2)!=0) throw new InvalidOperationException("Invalid flash image load"); } }
        public void Write(uint address, uint width, uint value, bool cpu)
        {
            if(cpu) {ulong data=value;CpuTransfer(address,width,true,ref data);return;}
            bool code;
            lock(sync) { if(write(native.State, address, width, value, cpu ? 1U : 0U) != 0) throw new InvalidOperationException($"Unsupported RAM write: 0x{address:X}"); code=codePages.Contains(address>>12); }
            if(code && !cpu) this.cpu?.ClearTranslationCache(); // Native stores already invalidate translated blocks at a safe point.
        }
        public uint ReadScb(long offset) { lock(sync) return native.ReadDoubleWord(offset); }
        public bool Contains(uint address) { return contains(address, 4) != 0; }
        public void Save(string path) { lock(sync) { if(save(native.State, path) != 0) throw new InvalidOperationException("Could not save memory/cache statistics"); } }
        public void WriteScb(long offset, uint value) { Synchronize(true);lock(sync) native.WriteDoubleWord(offset, value); }
        public void Reset() { lock(sync) native.Reset(); }
        private readonly object sync = new object();
        private readonly DaisyNative native;
        private readonly ReadMemory read;
        private readonly WriteMemory write;
        private readonly ContainsMemory contains;
        private readonly SaveNative save;
        private readonly HashSet<uint> codePages=new HashSet<uint>();
        private DaisyCortexM cpu;
        private readonly IMachine machine;
        private readonly SyncNative synchronize;
        private readonly AccessNative access;
        private readonly DebtNative debt;
        private readonly RegNative register;
        private readonly PortNative port;
        private readonly LoadNative load;
        private readonly TimeNative advance;
        private readonly IrqNative flashIrq;
        private readonly AbandonNative abandon;
        private readonly IrqNative waitInterruptible;
    }

    public sealed class DaisyMemory : IExecutableIO
    {
        public DaisyMemory(IMachine machine, DaisyArchitecture architecture, uint baseAddress, long size,bool readOnly=false) { this.machine = machine; this.architecture = architecture; this.baseAddress = baseAddress; Size = size; this.readOnly=readOnly; }
        private uint Address(long offset) { if(offset < 0 || offset >= Size) throw new InvalidOperationException("RAM offset out of range"); return checked(baseAddress + (uint)offset); }
        private uint Mode() { if(machine.SystemBus.TryGetCurrentContextState<DaisyCortexM.FetchState>(out var _,out var state) && state.InstructionFetch) return 2; return machine.SystemBus.TryGetCurrentCPU(out var _) ? 1U:0U; }
        public byte ReadByte(long offset) { return (byte)architecture.ReadMode(Address(offset), 1, Mode()); }
        public ushort ReadWord(long offset) { return (ushort)architecture.ReadMode(Address(offset), 2, Mode()); }
        public uint ReadDoubleWord(long offset) { return architecture.ReadMode(Address(offset), 4, Mode()); }
        public ulong ReadQuadWord(long offset) {if(Mode()==1) {ulong data=0;architecture.CpuTransfer(Address(offset),8,false,ref data);return data;}return ReadDoubleWord(offset)|((ulong)ReadDoubleWord(offset+4)<<32);}
        public void WriteQuadWord(long offset,ulong value) {if(machine.SystemBus.TryGetCurrentCPU(out var _)) architecture.CpuTransfer(Address(offset),8,true,ref value);else {WriteDoubleWord(offset,(uint)value);WriteDoubleWord(offset+4,(uint)(value>>32));}}
        public byte[] ReadBytes(long offset,int count,IPeripheral context=null) { var result=new byte[count]; for(int i=0;i<count;i++) result[i]=ReadByte(offset+i); return result; }
        public void WriteBytes(long offset,byte[] data,int startingIndex,int count,IPeripheral context=null) { for(int i=0;i<count;i++) WriteByte(offset+i,data[startingIndex+i]); }
        private void Write(long offset,uint width,uint value) { bool cpu=machine.SystemBus.TryGetCurrentCPU(out var _); if(readOnly && architecture.ImageLoading) architecture.LoadFlash(Address(offset),width,value); else architecture.Write(Address(offset),width,value,cpu); }
        public void WriteByte(long offset, byte value) { Write(offset,1,value); }
        public void WriteWord(long offset, ushort value) { Write(offset,2,value); }
        public void WriteDoubleWord(long offset, uint value) { Write(offset,4,value); }
        public void Reset() {} // CPU/peripheral reset does not erase physical RAM.
        public long Size { get; }
        private readonly uint baseAddress;
        private readonly IMachine machine;
        private readonly DaisyArchitecture architecture;
        private readonly bool readOnly;
    }

    public sealed class DaisyFlashController : IDoubleWordPeripheral,IWordPeripheral,IBytePeripheral,IKnownSize,IDisposable
    {
        public DaisyFlashController(IMachine machine,DaisyArchitecture architecture) {this.architecture=architecture;clock=machine.ObtainManagedThread(Tick,1000000);clock.Start();}
        private void Tick() {architecture.Advance();UpdateIrq();}
        private void UpdateIrq() {IRQ.Set(architecture.FlashIrq);}
        public uint ReadDoubleWord(long offset) {return architecture.Register(0,offset,0,false);}
        public void Save(string path) {architecture.Save(path);}
        public ushort ReadWord(long offset) {return checked((ushort)architecture.Register(0,offset,0,false,2));}
        public byte ReadByte(long offset) {return checked((byte)architecture.Register(0,offset,0,false,1));}
        public void WriteDoubleWord(long offset,uint value) {architecture.Register(0,offset,value,true);UpdateIrq();}
        public void WriteWord(long offset,ushort value) {architecture.Register(0,offset,value,true,2);UpdateIrq();}
        public void WriteByte(long offset,byte value) {architecture.Register(0,offset,value,true,1);UpdateIrq();}
        public GPIO IRQ {get;}=new GPIO();
        public long Size=>0x1000;
        public void Reset() {IRQ.Unset();}
        public void Dispose() {clock.Dispose();}
        private readonly DaisyArchitecture architecture;
        private readonly IManagedThread clock;
    }
    public sealed class DaisyAxi : IDoubleWordPeripheral,IKnownSize {
        public DaisyAxi(DaisyArchitecture architecture) {this.architecture=architecture;}
        public uint ReadDoubleWord(long offset)=>architecture.Register(1,offset,0,false);
        public void WriteDoubleWord(long offset,uint value)=>architecture.Register(1,offset,value,true);
        public long Size=>0x100000;
        public void Reset() {}
        private readonly DaisyArchitecture architecture;
    }

    // Reimplement the bus interface while retaining Renode's NVIC/MPU execution.
    public sealed class DaisyNvic : NVIC, IDoubleWordPeripheral
    {
        public DaisyNvic(IMachine machine, DaisyArchitecture architecture, byte priorityMask = 0xF0) : base(machine, priorityMask: priorityMask) { this.machine=machine; this.architecture = architecture; FilterCcrDiv0Write = false; }
        public new uint ReadDoubleWord(long offset)
        {
            if(offset == 0xd78 || offset == 0xd7c || offset == 0xd80 || offset == 0xd84) return architecture.ReadScb(offset);
            if(offset == 0xd14) return (base.ReadDoubleWord(offset) & ~(3U << 16)) | (architecture.ReadScb(offset) & (3U << 16));
            return base.ReadDoubleWord(offset);
        }
        public new void WriteDoubleWord(long offset, uint value)
        {
            if(offset == 0xd14) { architecture.WriteScb(offset, value); base.WriteDoubleWord(offset, value & ~(3U << 16)); ((DaisyCortexM)machine["sysbus.cpu"]).ClearTranslationCache(); return; }
            if(offset == 0xd84 || (offset >= 0xf50 && offset <= 0xf74)) { architecture.WriteScb(offset, value); if(offset!=0xd84) ((DaisyCortexM)machine["sysbus.cpu"]).ClearTranslationCache(); return; }
            if(offset >= 0xd94 && offset <= 0xdb8) architecture.WriteScb(offset, value);
            base.WriteDoubleWord(offset, value);
            if(offset==0xdfc) ((DaisyRcc)machine["sysbus.rcc"]).TraceEnable((value&(1U<<24))!=0);
        }
        public new void Reset() { base.Reset(); architecture.Reset(); }
        public void SaveMemory(string path) { architecture.Save(path); }
        public DaisyArchitecture Architecture => architecture;
        private readonly DaisyArchitecture architecture;
        private readonly IMachine machine;
    }
}
