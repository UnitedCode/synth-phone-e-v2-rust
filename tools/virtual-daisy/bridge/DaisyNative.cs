// Renode's mandatory managed peripheral interface; register behavior lives in Rust.
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Collections.Generic;
using Antmicro.Renode.Core;
using Antmicro.Renode.Peripherals.Analog;
using Antmicro.Renode.Peripherals.DMA;
using Antmicro.Renode.Peripherals.I2C;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public class DaisyNative : IDoubleWordPeripheral, IKnownSize
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr New(uint kind);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong Read(IntPtr state, ulong offset);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint Write(IntPtr state, ulong offset, uint value);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void ResetNative(IntPtr state);

        public DaisyNative(string libraryPath, uint kind, long size)
        {
            var library = NativeLibrary.Load(libraryPath);
            var create = Marshal.GetDelegateForFunctionPointer<New>(NativeLibrary.GetExport(library, "daisy_new"));
            read = Marshal.GetDelegateForFunctionPointer<Read>(NativeLibrary.GetExport(library, "daisy_read"));
            write = Marshal.GetDelegateForFunctionPointer<Write>(NativeLibrary.GetExport(library, "daisy_write"));
            reset = Marshal.GetDelegateForFunctionPointer<ResetNative>(NativeLibrary.GetExport(library, "daisy_reset"));
            state = create(kind);
            Kind = kind;
            Size = size;
        }
        public uint ReadDoubleWord(long offset)
        {
            var result = read(state, (ulong)offset);
            if((result >> 32) != 0) throw new InvalidOperationException($"Unsupported Daisy read: kind={Kind}, offset=0x{offset:X}");
            return (uint)result;
        }
        public bool TryRead(long offset, out uint value)
        {
            var result = read(state, (ulong)offset);
            value = (uint)result;
            return (result >> 32) == 0;
        }
        public bool TryWrite(long offset, uint value) { return write(state, (ulong)offset, value) == 0; }
        public void WriteDoubleWord(long offset, uint value)
        {
            if(write(state, (ulong)offset, value) != 0) throw new InvalidOperationException($"Unsupported Daisy write: kind={Kind}, offset=0x{offset:X}, value=0x{value:X}");
        }
        public void Reset() { reset(state); }
        public long Size { get; }
        public uint Kind { get; }
        public IntPtr State { get { return state; } }
        private readonly IntPtr state;
        private readonly Read read;
        private readonly Write write;
        private readonly ResetNative reset;
    }

    public class DaisySyscfg : IDoubleWordPeripheral, IKnownSize, INumberedGPIOOutput, ILocalGPIOReceiver
    {
        public DaisySyscfg(string libraryPath)
        {
            extra = new DaisyNative(libraryPath, 2, 0x400);
        }
        public uint ReadDoubleWord(long offset) { uint value; return extra.TryRead(offset, out value) ? value : original.ReadDoubleWord(offset); }
        public void WriteDoubleWord(long offset, uint value) { if(!extra.TryWrite(offset, value)) original.WriteDoubleWord(offset, value); }
        public void Reset() { original.Reset(); extra.Reset(); }
        public long Size { get { return 0x400; } }
        public IReadOnlyDictionary<int, IGPIO> Connections { get { return original.Connections; } }
        public IGPIOReceiver GetLocalReceiver(int index) { return original.GetLocalReceiver(index); }
        private readonly STM32_SYSCFG original = new STM32_SYSCFG();
        private readonly DaisyNative extra;
    }

    public class DaisyAdc : IDoubleWordPeripheral, IKnownSize
    {
        public DaisyAdc(IMachine machine, string libraryPath, double referenceVoltage, uint externalEventFrequency, int dmaChannel = 0, IDMA dmaPeripheral = null)
        {
            original = new STM32H7_ADC(machine, referenceVoltage, externalEventFrequency, dmaChannel, dmaPeripheral);
            extra = new DaisyNative(libraryPath, 3, 0x400);
        }
        public uint ReadDoubleWord(long offset) { uint value; return extra.TryRead(offset, out value) ? value : original.ReadDoubleWord(offset); }
        public void WriteDoubleWord(long offset, uint value) { if(!extra.TryWrite(offset, value)) original.WriteDoubleWord(offset, value); }
        public void Reset() { original.Reset(); extra.Reset(); }
        public GPIO IRQ { get { return original.IRQ; } }
        public long Size { get { return 0x400; } }
        private readonly STM32H7_ADC original;
        private readonly DaisyNative extra;
    }

    public class DaisyI2c : II2CPeripheral, IGPIOReceiver
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint WriteNative(IntPtr state, byte[] bytes, UIntPtr length);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint ControlNative(IntPtr state, uint high);
        public DaisyI2c(string libraryPath, uint kind)
        {
            var library = NativeLibrary.Load(libraryPath);
            native = new DaisyNative(libraryPath, kind, 0x400);
            write = Marshal.GetDelegateForFunctionPointer<WriteNative>(NativeLibrary.GetExport(library, "daisy_i2c_write"));
            control = Marshal.GetDelegateForFunctionPointer<ControlNative>(NativeLibrary.GetExport(library, "daisy_codec_control"));
        }
        public void Write(byte[] bytes) { lock(sync) { if(write(native.State, bytes, (UIntPtr)bytes.Length) != 0) throw new InvalidOperationException("Unsupported I2C transaction"); } }
        public byte[] Read(int count) { throw new InvalidOperationException("Read from write-only Daisy codec/display control model"); }
        public void FinishTransmission() {}
        public void Reset() { lock(sync) native.Reset(); }
        public uint ActiveMask { get { lock(sync) return native.ReadDoubleWord(0x100); } }
        public uint WithState(Func<IntPtr,uint> operation) { lock(sync) return operation(native.State); }
        public void OnGPIO(int pin, bool high) { lock(sync) { if(pin != 0 || control(native.State, high ? 1U : 0U) != 0) throw new InvalidOperationException("Unsupported codec control signal"); } }
        private readonly object sync = new object();
        private readonly DaisyNative native;
        private readonly WriteNative write;
        private readonly ControlNative control;
    }

    public class DaisyDma : IDoubleWordPeripheral, IKnownSize, IGPIOReceiver, INumberedGPIOOutput
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong Request(IntPtr state, uint channel);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint Irqs(IntPtr state);
        public DaisyDma(IMachine machine, string libraryPath, DaisyArchitecture architecture)
        {
            this.machine = machine;
            this.architecture = architecture;
            native = new DaisyNative(libraryPath, 6, 0x400);
            var library = NativeLibrary.Load(libraryPath);
            request = Marshal.GetDelegateForFunctionPointer<Request>(NativeLibrary.GetExport(library, "daisy_dma_request"));
            irqs = Marshal.GetDelegateForFunctionPointer<Irqs>(NativeLibrary.GetExport(library, "daisy_dma_irqs"));
            var connections = new Dictionary<int, IGPIO>();
            for(int i = 0; i < 8; i++) connections[i] = new GPIO();
            Connections = connections;
        }
        public uint ReadDoubleWord(long offset) { lock(sync) return native.ReadDoubleWord(offset); }
        public void WriteDoubleWord(long offset, uint value) { lock(sync) { native.WriteDoubleWord(offset, value); UpdateIrqs(); } }
        public void OnGPIO(int number, bool value)
        {
            if(!value) return;
            lock(sync)
            {
                var transfer = request(native.State, (uint)number);
                if(transfer == ulong.MaxValue) throw new InvalidOperationException("Invalid DMA audio request");
                if(transfer != 0) {
                    uint source = (uint)(transfer >> 32), destination = (uint)transfer;
                    uint word = architecture.Contains(source) ? architecture.Read(source, 4, false) : machine.SystemBus.ReadDoubleWord(source);
                    if(architecture.Contains(destination)) architecture.Write(destination, 4, word, false); else machine.SystemBus.WriteDoubleWord(destination, word);
                }
                UpdateIrqs();
            }
        }
        private void UpdateIrqs() { uint mask = irqs(native.State); for(int i = 0; i < 8; i++) Connections[i].Set((mask & (1U << i)) != 0); }
        public void Reset() { lock(sync) { native.Reset(); UpdateIrqs(); } }
        public long Size { get { return 0x400; } }
        public IReadOnlyDictionary<int, IGPIO> Connections { get; }
        private readonly object sync = new object();
        private readonly IMachine machine;
        private readonly DaisyArchitecture architecture;
        private readonly DaisyNative native;
        private readonly Request request;
        private readonly Irqs irqs;
    }

    public class DaisySai : IDoubleWordPeripheral, IKnownSize, IDisposable
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint CodecTickNative(IntPtr state, IntPtr codec, uint idealLine);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void FaultNative(IntPtr state, uint dropRx);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint ConfigureNative(IntPtr state, uint rxChannel);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint RateNative(IntPtr state, double rate);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint Requests(IntPtr state);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void Input(IntPtr state, float[] samples, UIntPtr frames);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint SaveNative(IntPtr state, [MarshalAs(UnmanagedType.LPUTF8Str)] string path, uint sampleRate);
        public DaisySai(IMachine machine, string libraryPath, DaisyI2c codec, DaisyRcc rcc, uint rxChannel)
        {
            this.codec = codec;
            this.rcc = rcc;
            this.rxChannel = rxChannel;
            native = new DaisyNative(libraryPath, 7, 0x400);
            var library = NativeLibrary.Load(libraryPath);
            var configure = Marshal.GetDelegateForFunctionPointer<ConfigureNative>(NativeLibrary.GetExport(library, "daisy_sai_configure"));
            if(configure(native.State, rxChannel) != 0) throw new InvalidOperationException("Unsupported SAI board wiring");
            codecTick = Marshal.GetDelegateForFunctionPointer<CodecTickNative>(NativeLibrary.GetExport(library, "daisy_sai_tick_codec"));
            setRate = Marshal.GetDelegateForFunctionPointer<RateNative>(NativeLibrary.GetExport(library, "daisy_sai_rate"));
            fault = Marshal.GetDelegateForFunctionPointer<FaultNative>(NativeLibrary.GetExport(library, "daisy_sai_fault"));
            requests = Marshal.GetDelegateForFunctionPointer<Requests>(NativeLibrary.GetExport(library, "daisy_sai_requests"));
            input = Marshal.GetDelegateForFunctionPointer<Input>(NativeLibrary.GetExport(library, "daisy_sai_input"));
            save = Marshal.GetDelegateForFunctionPointer<SaveNative>(NativeLibrary.GetExport(library, "daisy_sai_save"));
            clock = machine.ObtainManagedThread(Tick, 1000000);
            clock.Start();
        }
        public uint ReadDoubleWord(long offset) { lock(sync) return native.ReadDoubleWord(offset); }
        public void WriteDoubleWord(long offset, uint value) { lock(sync) { native.WriteDoubleWord(offset, value); if(offset==4) cr1=native.ReadDoubleWord(4); if(offset==0xc) frcr=native.ReadDoubleWord(0xc); if(offset == 4 || offset == 0x24) PumpRequests(); } }
        private void Tick()
        {
            // RCC reset can call Reset(). Do not hold the SAI lock while
            // acquiring RCC's lock in the opposite direction.
            uint frames = rcc.Frames(cr1, frcr, 1000);
            double rate = frames==0 ? 0 : rcc.ExactRate(cr1,frcr);
            lock(sync)
            {
                if(frames!=0 && setRate(native.State,rate)!=0) throw new InvalidOperationException("Invalid SAI frame rate");
                for(uint frame=0;frame<frames;frame++) { if(codec.WithState(state=>codecTick(native.State,state,IdealLineInput ? 1U:0U))!=0) throw new InvalidOperationException("Unsupported codec audio path"); PumpRequests(); }
            }
        }
        private void PumpRequests() { Pump(rxChannel == 0 ? DmaA : DmaB, 1U << (int)rxChannel, 2); Pump(rxChannel == 0 ? DmaB : DmaA, 1U << (int)(1 - rxChannel), 8); }
        private void Pump(GPIO gpio, uint mask, int words) { for(int i = 0; i < words && (requests(native.State) & mask) != 0; i++) { gpio.Set(true); gpio.Set(false); } }
        public void LoadInput(float[] samples) { lock(sync) input(native.State, samples, (UIntPtr)(samples.Length / 2)); }
        public void Save(string path) { uint rate=rcc.Rate(cr1,frcr); lock(sync) { if(save(native.State,path,rate)!=0) throw new IOException("Could not save virtual SAI output"); } rcc.Save(cr1,frcr,path+".clock.txt"); }
        public void Reset() { lock(sync) { native.Reset(); cr1=0; frcr=0; } }
        public void Dispose() { clock.Dispose(); }
        public long Size { get { return 0x400; } }
        public GPIO DmaA { get; } = new GPIO();
        public GPIO DmaB { get; } = new GPIO();
        public bool SuppressReceive { set { lock(sync) fault(native.State, value ? 1U : 0U); } }
        public bool IdealLineInput { get; set; }
        private readonly object sync = new object();
        private readonly DaisyNative native;
        private readonly DaisyI2c codec;
        private readonly DaisyRcc rcc;
        private readonly uint rxChannel;
        private readonly IManagedThread clock;
        private readonly CodecTickNative codecTick;
        private readonly RateNative setRate;
        private uint cr1, frcr;
        private readonly FaultNative fault;
        private readonly Requests requests;
        private readonly Input input;
        private readonly SaveNative save;
    }

    public class DaisyQspi : DaisyNative, IBytePeripheral, IWordPeripheral
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong ReadWidth(IntPtr state, ulong offset, uint width);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint WriteWidth(IntPtr state, ulong offset, uint value, uint width);
        public DaisyQspi(string libraryPath) : base(libraryPath, 8, 0x400)
        {
            var library = NativeLibrary.Load(libraryPath);
            readWidth = Marshal.GetDelegateForFunctionPointer<ReadWidth>(NativeLibrary.GetExport(library, "daisy_read_width"));
            writeWidth = Marshal.GetDelegateForFunctionPointer<WriteWidth>(NativeLibrary.GetExport(library, "daisy_write_width"));
        }
        private uint Read(long offset, uint width) { ulong value = readWidth(State, (ulong)offset, width); if((value >> 32) != 0) throw new InvalidOperationException("Unsupported QSPI read"); return (uint)value; }
        private void Write(long offset, uint value, uint width) { if(writeWidth(State, (ulong)offset, value, width) != 0) throw new InvalidOperationException("Unsupported QSPI write"); }
        public byte ReadByte(long offset) { return (byte)Read(offset, 1); }
        public ushort ReadWord(long offset) { return (ushort)Read(offset, 2); }
        public void WriteByte(long offset, byte value) { Write(offset, value, 1); }
        public void WriteWord(long offset, ushort value) { Write(offset, value, 2); }
        private readonly ReadWidth readWidth;
        private readonly WriteWidth writeWidth;
    }
    public class DaisyGpio : IDoubleWordPeripheral, IKnownSize, INumberedGPIOOutput, IGPIOReceiver, ILocalGPIOReceiver
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void Drive(IntPtr state, uint pin, uint high);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void Alternate(IntPtr state, uint pin, uint af, uint high);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint Straps(IntPtr state, uint grounded);
        public DaisyGpio(string libraryPath, uint port)
        {
            native = new DaisyNative(libraryPath, 100 + port, 0x400);
            var library = NativeLibrary.Load(libraryPath);
            drive = Marshal.GetDelegateForFunctionPointer<Drive>(NativeLibrary.GetExport(library, "daisy_gpio_drive"));
            alternate = Marshal.GetDelegateForFunctionPointer<Alternate>(NativeLibrary.GetExport(library, "daisy_gpio_alternate"));
            straps = Marshal.GetDelegateForFunctionPointer<Straps>(NativeLibrary.GetExport(library, "daisy_revision_straps"));
            var connections = new Dictionary<int, IGPIO>();
            for(int pin = 0; pin < 16; pin++) connections[pin] = new GPIO();
            Connections = connections;
        }
        public uint ReadDoubleWord(long offset) { lock(sync) return native.ReadDoubleWord(offset); }
        public void WriteDoubleWord(long offset, uint value) { lock(sync) { native.WriteDoubleWord(offset, value); Update(); } }
        public void OnGPIO(int pin, bool high) { lock(sync) { drive(native.State, (uint)pin, high ? 1U : 0U); Update(); } }
        public void ConfigureStraps(uint grounded) { lock(sync) { if(straps(native.State, grounded) != 0) throw new InvalidOperationException("Invalid board straps"); Update(); } }
        private void Update() { uint levels = native.ReadDoubleWord(0x10); for(int pin = 0; pin < 16; pin++) Connections[pin].Set((levels & (1U << pin)) != 0); }
        public void Reset() { lock(sync) { native.Reset(); Update(); } }
        public long Size { get { return 0x400; } }
        public IReadOnlyDictionary<int, IGPIO> Connections { get; }
        private readonly DaisyNative native;
        private readonly Drive drive;
        private readonly Alternate alternate;
        private readonly Straps straps;
        public IGPIOReceiver GetLocalReceiver(int pin) { return new AlternateReceiver(this, pin); }
        private sealed class AlternateReceiver : IGPIOReceiver { public AlternateReceiver(DaisyGpio owner, int pin) { this.owner = owner; this.pin = pin; } public void OnGPIO(int af, bool high) { lock(owner.sync) { owner.alternate(owner.native.State, (uint)pin, (uint)af, high ? 1U : 0U); owner.Update(); } } public void Reset() {} private readonly DaisyGpio owner; private readonly int pin; }
        private readonly object sync = new object();
    }
}
