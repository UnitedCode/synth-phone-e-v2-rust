// Required Renode interfaces; RAM, MPU cache policy and cache maintenance are Rust.
using System;
using System.Runtime.InteropServices;
using System.Collections.Generic;
using Antmicro.Renode.Core;
using Antmicro.Renode.Peripherals;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.IRQControllers;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public sealed class DaisyArchitecture : IPeripheral
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong ReadMemory(IntPtr state, uint address, uint width, uint cpu);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint WriteMemory(IntPtr state, uint address, uint width, uint value, uint cpu);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint ContainsMemory(uint address, uint width);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint SaveNative(IntPtr state, [MarshalAs(UnmanagedType.LPUTF8Str)] string path);
        public DaisyArchitecture(string libraryPath)
        {
            native = new DaisyNative(libraryPath, 12, 0x1000);
            var library = NativeLibrary.Load(libraryPath);
            read = Marshal.GetDelegateForFunctionPointer<ReadMemory>(NativeLibrary.GetExport(library, "daisy_memory_read"));
            write = Marshal.GetDelegateForFunctionPointer<WriteMemory>(NativeLibrary.GetExport(library, "daisy_memory_write"));
            contains = Marshal.GetDelegateForFunctionPointer<ContainsMemory>(NativeLibrary.GetExport(library, "daisy_memory_is_ram"));
            save = Marshal.GetDelegateForFunctionPointer<SaveNative>(NativeLibrary.GetExport(library, "daisy_memory_save"));
            RuntimeFetchPointer=NativeLibrary.GetExport(library,"daisy_runtime_fetch");
        }
        public uint Read(uint address, uint width, bool cpu)
        {
            return ReadMode(address,width,cpu ? 1U:0U);
        }
        public uint ReadMode(uint address,uint width,uint mode) { lock(sync) { if(mode==2) codePages.Add(address>>12); ulong value=read(native.State,address,width,mode); if(value>>32!=0) throw new InvalidOperationException($"Unsupported RAM access: 0x{address:X}"); return (uint)value; } }
        public void AttachCPU(DaisyCortexM cpu) { this.cpu=cpu; }
        public IntPtr NativeState => native.State;
        public IntPtr RuntimeFetchPointer { get; }
        public bool ImageLoading { get; set; }
        public void LoadFlash(uint address,uint width,uint value) { lock(sync) { if(write(native.State,address,width,value,2)!=0) throw new InvalidOperationException("Invalid flash image load"); } }
        public void Write(uint address, uint width, uint value, bool cpu)
        {
            bool code;
            lock(sync) { if(write(native.State, address, width, value, cpu ? 1U : 0U) != 0) throw new InvalidOperationException($"Unsupported RAM write: 0x{address:X}"); code=codePages.Contains(address>>12); }
            if(code && !cpu) this.cpu?.ClearTranslationCache(); // Native stores already invalidate translated blocks at a safe point.
        }
        public uint ReadScb(long offset) { lock(sync) return native.ReadDoubleWord(offset); }
        public bool Contains(uint address) { return contains(address, 4) != 0; }
        public void Save(string path) { lock(sync) { if(save(native.State, path) != 0) throw new InvalidOperationException("Could not save memory/cache statistics"); } }
        public void WriteScb(long offset, uint value) { lock(sync) native.WriteDoubleWord(offset, value); }
        public void Reset() { lock(sync) native.Reset(); }
        private readonly object sync = new object();
        private readonly DaisyNative native;
        private readonly ReadMemory read;
        private readonly WriteMemory write;
        private readonly ContainsMemory contains;
        private readonly SaveNative save;
        private readonly HashSet<uint> codePages=new HashSet<uint>();
        private DaisyCortexM cpu;
    }

    public sealed class DaisyMemory : IExecutableIO
    {
        public DaisyMemory(IMachine machine, DaisyArchitecture architecture, uint baseAddress, long size,bool readOnly=false) { this.machine = machine; this.architecture = architecture; this.baseAddress = baseAddress; Size = size; this.readOnly=readOnly; }
        private uint Address(long offset) { if(offset < 0 || offset >= Size) throw new InvalidOperationException("RAM offset out of range"); return checked(baseAddress + (uint)offset); }
        private uint Mode() { if(machine.SystemBus.TryGetCurrentContextState<DaisyCortexM.FetchState>(out var _,out var state) && state.InstructionFetch) return 2; return machine.SystemBus.TryGetCurrentCPU(out var _) ? 1U:0U; }
        public byte ReadByte(long offset) { return (byte)architecture.ReadMode(Address(offset), 1, Mode()); }
        public ushort ReadWord(long offset) { return (ushort)architecture.ReadMode(Address(offset), 2, Mode()); }
        public uint ReadDoubleWord(long offset) { return architecture.ReadMode(Address(offset), 4, Mode()); }
        public ulong ReadQuadWord(long offset) { return ReadDoubleWord(offset)|((ulong)ReadDoubleWord(offset+4)<<32); }
        public void WriteQuadWord(long offset,ulong value) { WriteDoubleWord(offset,(uint)value); WriteDoubleWord(offset+4,(uint)(value>>32)); }
        public byte[] ReadBytes(long offset,int count,IPeripheral context=null) { var result=new byte[count]; for(int i=0;i<count;i++) result[i]=ReadByte(offset+i); return result; }
        public void WriteBytes(long offset,byte[] data,int startingIndex,int count,IPeripheral context=null) { for(int i=0;i<count;i++) WriteByte(offset+i,data[startingIndex+i]); }
        private void Write(long offset,uint width,uint value) { bool cpu=machine.SystemBus.TryGetCurrentCPU(out var _); if(readOnly) { if(cpu || !architecture.ImageLoading) throw new InvalidOperationException("Flash programming is not implemented"); architecture.LoadFlash(Address(offset),width,value); } else architecture.Write(Address(offset),width,value,cpu); }
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

    // Keep upstream read/status/ACR interfaces; reject unmodeled programming,
    // erase and option commands instead of mutating a disconnected backing bank.
    public sealed class DaisyReadOnlyFlashController : Antmicro.Renode.Peripherals.MTD.STM32H7_FlashController
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint SaveNative(IntPtr clock,uint acr,[MarshalAs(UnmanagedType.LPUTF8Str)] string path);
        public DaisyReadOnlyFlashController(IMachine machine,Antmicro.Renode.Peripherals.Memory.MappedMemory flash1,Antmicro.Renode.Peripherals.Memory.MappedMemory flash2,DaisyRcc rcc,string libraryPath):base(machine,flash1,flash2) { this.rcc=rcc; var library=NativeLibrary.Load(libraryPath); save=Marshal.GetDelegateForFunctionPointer<SaveNative>(NativeLibrary.GetExport(library,"daisy_flash_bus_save")); }
        public override void WriteDoubleWord(long offset,uint value) { if(offset!=0) throw new InvalidOperationException("Flash program/erase/options are not modeled"); base.WriteDoubleWord(offset,value); }
        public void Save(string path) { if(save(rcc.NativeState,base.ReadDoubleWord(0),path)!=0) throw new InvalidOperationException("Invalid flash bus timing"); }
        private readonly DaisyRcc rcc;
        private readonly SaveNative save;
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
