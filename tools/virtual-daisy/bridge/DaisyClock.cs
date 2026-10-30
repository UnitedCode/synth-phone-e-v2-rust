// Renode clock/registration adapters. Divider arithmetic and CYCCNT live in Rust.
using System;
using System.Runtime.InteropServices;
using System.Collections.Generic;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure;
using Antmicro.Renode.Peripherals;
using Antmicro.Renode.Peripherals.Bus;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public sealed class DaisyRcc : IDoubleWordPeripheral, IKnownSize, IRegisterablePeripheral<IPeripheral, EnumRegistrationPoint<STM32H7_RCC.RegistrationPeripheralAlias>>
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong AdvanceNative(IntPtr state, uint cr1, uint frcr, ulong ns);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate double RateNative(IntPtr state, uint cr1, uint frcr);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong DomainNative(IntPtr state,uint domain);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong DwtNative(IntPtr state, ulong offset, uint value, uint write, ulong now);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint SaveNative(IntPtr state, uint cr1, uint frcr, [MarshalAs(UnmanagedType.LPUTF8Str)] string path);
        public DaisyRcc(IMachine machine, string libraryPath, IHasDivisibleFrequency systick = null)
        {
            this.machine=machine;
            this.systick=systick;
            original=new STM32H7_RCC(machine,systick,hseFrequency:16000000);
            native=new DaisyNative(libraryPath,13,0x400);
            var library=NativeLibrary.Load(libraryPath);
            advance=Marshal.GetDelegateForFunctionPointer<AdvanceNative>(NativeLibrary.GetExport(library,"daisy_clock_advance"));
            rate=Marshal.GetDelegateForFunctionPointer<RateNative>(NativeLibrary.GetExport(library,"daisy_clock_rate"));
            domain=Marshal.GetDelegateForFunctionPointer<DomainNative>(NativeLibrary.GetExport(library,"daisy_clock_domain"));
            dwt=Marshal.GetDelegateForFunctionPointer<DwtNative>(NativeLibrary.GetExport(library,"daisy_clock_dwt"));
            save=Marshal.GetDelegateForFunctionPointer<SaveNative>(NativeLibrary.GetExport(library,"daisy_clock_save"));
        }
        private bool Owned(long offset) { return offset==0x2c || (offset>=0x34 && offset<=0x44) || offset==0x4c || offset==0x50; }
        public uint ReadDoubleWord(long offset) { lock(sync) return Owned(offset) ? native.ReadDoubleWord(offset) : original.ReadDoubleWord(offset); }
        public void WriteDoubleWord(long offset, uint value)
        {
            lock(sync) {
                Dwt(0,0,2);
                native.WriteDoubleWord(offset,value);
                if(!Owned(offset)) { original.WriteDoubleWord(offset,value); native.WriteDoubleWord(offset,original.ReadDoubleWord(offset)); }
                PropagateClocks();
            }
        }
        public uint Frames(uint cr1,uint frcr,ulong ns) { lock(sync) { ulong result=advance(native.State,cr1,frcr,ns); if(result>>32 != 0) throw new InvalidOperationException("Unsupported SAI clock configuration"); return (uint)result; } }
        public uint Rate(uint cr1,uint frcr) { lock(sync) { double hz=rate(native.State,cr1,frcr); if(!double.IsFinite(hz) || hz<1 || hz>384000) throw new InvalidOperationException("Invalid capture rate"); return checked((uint)Math.Round(hz)); } }
        public double ExactRate(uint cr1,uint frcr) { lock(sync) return rate(native.State,cr1,frcr); }
        public uint Dwt(ulong offset,uint value,uint write) {
            lock(sync) {
                if(machine.SystemBus.TryGetCurrentCPU(out var cpu)) cpu.SyncTime();
                ulong result=dwt(native.State,offset,value,write,machine.ElapsedVirtualTime.TimeElapsed.TotalNanoseconds);
                if(result>>32 != 0) throw new InvalidOperationException("Unsupported DWT/core clock configuration");
                return (uint)result;
            }
        }
        public void TraceEnable(bool enabled) { Dwt(0,enabled ? 1U:0U,3); }
        public void Save(uint cr1,uint frcr,string path) { lock(sync) { if(save(native.State,cr1,frcr,path)!=0) throw new InvalidOperationException("Cannot save clock evidence"); } }
        public void Register(IPeripheral peripheral,EnumRegistrationPoint<STM32H7_RCC.RegistrationPeripheralAlias> point) {
            original.Register(peripheral,point);
            string name=point.Name.ToString();
            uint group=name is "TIM2" or "TIM3" or "TIM4" or "TIM5" or "TIM6" or "TIM7" or "TIM12" or "TIM13" or "TIM14" ? 6U : name is "TIM1" or "TIM8" or "TIM15" or "TIM16" or "TIM17" ? 7U : 0U;
            if(group!=0 && peripheral is IHasFrequency) timers.Add(peripheral,group);
        }
        public void Unregister(IPeripheral peripheral) { original.Unregister(peripheral); timers.Remove(peripheral); }
        private void PropagateClocks() {
            ulong core=domain(native.State,0);
            if(core==ulong.MaxValue) throw new InvalidOperationException("Invalid core clock");
            if(core!=0 && systick!=null) systick.Frequency=core;
            foreach(var timer in timers) { ulong hz=domain(native.State,timer.Value); if(hz==ulong.MaxValue) throw new InvalidOperationException("Invalid timer clock"); if(hz!=0) ((IHasFrequency)timer.Key).Frequency=hz; }
        }
        public void Reset() { lock(sync) { original.Reset(); native.Reset(); } }
        public long Size => 0x400;
        public IntPtr NativeState => native.State;
        private readonly object sync=new object();
        private readonly IMachine machine;
        private readonly STM32H7_RCC original;
        private readonly DaisyNative native;
        private readonly AdvanceNative advance;
        private readonly RateNative rate;
        private readonly DomainNative domain;
        private readonly IHasDivisibleFrequency systick;
        private readonly Dictionary<IPeripheral,uint> timers=new Dictionary<IPeripheral,uint>();
        private readonly DwtNative dwt;
        private readonly SaveNative save;
    }
    public sealed class DaisyDwt : IDoubleWordPeripheral, IKnownSize
    {
        public DaisyDwt(IMachine machine,DaisyRcc rcc) { this.rcc=rcc; original=new DWT(machine,1); }
        public uint ReadDoubleWord(long offset) { return offset==0 || offset==4 || offset==0xfb4 ? rcc.Dwt((ulong)offset,0,0) : original.ReadDoubleWord(offset); }
        public void WriteDoubleWord(long offset,uint value) { if(offset==0 || offset==4 || offset==0xfb0) rcc.Dwt((ulong)offset,value,1); else original.WriteDoubleWord(offset,value); }
        public void Reset() { original.Reset(); }
        public long Size => 0x1000;
        private readonly DaisyRcc rcc;
        private readonly DWT original;
    }
}
