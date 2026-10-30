// Required Renode interfaces; RAM, MPU cache policy and cache maintenance are Rust.
using System;
using System.Runtime.InteropServices;
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
        }
        public uint Read(uint address, uint width, bool cpu)
        {
            lock(sync) { ulong value = read(native.State, address, width, cpu ? 1U : 0U); if((value >> 32) != 0) throw new InvalidOperationException($"Unsupported RAM read: 0x{address:X}"); return (uint)value; }
        }
        public void Write(uint address, uint width, uint value, bool cpu)
        {
            lock(sync) { if(write(native.State, address, width, value, cpu ? 1U : 0U) != 0) throw new InvalidOperationException($"Unsupported RAM write: 0x{address:X}"); }
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
    }

    public sealed class DaisyMemory : IBytePeripheral, IWordPeripheral, IDoubleWordPeripheral, IKnownSize
    {
        public DaisyMemory(IMachine machine, DaisyArchitecture architecture, uint baseAddress, long size) { this.machine = machine; this.architecture = architecture; this.baseAddress = baseAddress; Size = size; }
        private uint Address(long offset) { if(offset < 0 || offset >= Size) throw new InvalidOperationException("RAM offset out of range"); return checked(baseAddress + (uint)offset); }
        public byte ReadByte(long offset) { return (byte)architecture.Read(Address(offset), 1, machine.SystemBus.TryGetCurrentCPU(out var _)); }
        public ushort ReadWord(long offset) { return (ushort)architecture.Read(Address(offset), 2, machine.SystemBus.TryGetCurrentCPU(out var _)); }
        public uint ReadDoubleWord(long offset) { return architecture.Read(Address(offset), 4, machine.SystemBus.TryGetCurrentCPU(out var _)); }
        public void WriteByte(long offset, byte value) { architecture.Write(Address(offset), 1, value, machine.SystemBus.TryGetCurrentCPU(out var _)); }
        public void WriteWord(long offset, ushort value) { architecture.Write(Address(offset), 2, value, machine.SystemBus.TryGetCurrentCPU(out var _)); }
        public void WriteDoubleWord(long offset, uint value) { architecture.Write(Address(offset), 4, value, machine.SystemBus.TryGetCurrentCPU(out var _)); }
        public void Reset() {} // CPU/peripheral reset does not erase physical RAM.
        public long Size { get; }
        private readonly uint baseAddress;
        private readonly IMachine machine;
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
            if(offset == 0xd14) { architecture.WriteScb(offset, value); base.WriteDoubleWord(offset, value & ~(3U << 16)); return; }
            if(offset == 0xd84 || (offset >= 0xf50 && offset <= 0xf74)) { architecture.WriteScb(offset, value); return; }
            if(offset >= 0xd94 && offset <= 0xdb8) architecture.WriteScb(offset, value);
            base.WriteDoubleWord(offset, value);
            if(offset==0xdfc) ((DaisyRcc)machine["sysbus.rcc"]).TraceEnable((value&(1U<<24))!=0);
        }
        public new void Reset() { base.Reset(); architecture.Reset(); }
        public void SaveMemory(string path) { architecture.Save(path); }
        private readonly DaisyArchitecture architecture;
        private readonly IMachine machine;
    }
}
