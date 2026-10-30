// Mandatory Renode CPU adapter. Bit 63 is supplied by our pinned native
// instruction-fetch tag; it is never inferred from PC, thread or TB state.
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Peripherals;
using Antmicro.Renode.Peripherals.CPU;
using Antmicro.Renode.Peripherals.IRQControllers;
using Antmicro.Renode.Utilities.Binding;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public sealed class DaisyCortexM : CortexM, IPeripheralWithTransactionState
    {
        public DaisyCortexM(string cpuType,IMachine machine,NVIC nvic,DaisyArchitecture architecture,uint? numberOfMPURegions=null)
            :base(cpuType,machine,nvic,numberOfMPURegions:numberOfMPURegions) {
            architecture.AttachCPU(this);
            tlibDaisySetRuntimeFetch(architecture.NativeState,architecture.RuntimeFetchPointer);
        }
        public new bool TryConvertUlongToStateObj(ulong? raw,out IContextState state)
        {
            state=null;
            if(!raw.HasValue) return false;
            var original=(ContextState)raw.Value;
            state=new FetchState { Privileged=original.Privileged,CpuSecure=original.CpuSecure,AttributionSecure=original.AttributionSecure,Semihosting=original.Semihosting,InstructionFetch=(raw.Value&(1UL<<63))!=0 };
            return true;
        }
        public new bool TryConvertStateObjToUlong(IContextState state,out ulong? raw)
        {
            if(!base.TryConvertStateObjToUlong(state,out raw)) return false;
            if(state is FetchState fetch && fetch.InstructionFetch) raw|=1UL<<63;
            return true;
        }
        public sealed class FetchState : ContextState { public bool InstructionFetch; }
        public uint FetchTagVersion => tlibDaisyFetchTagVersion();
        public void DisableRuntimeFetching() { tlibDaisySetRuntimeFetch(IntPtr.Zero,IntPtr.Zero); }
        // Assigned by Renode's native binder, as required by its import API.
#pragma warning disable CS0649
        [Import] private readonly Func<uint> tlibDaisyFetchTagVersion;
        [Import] private readonly Action<IntPtr,IntPtr> tlibDaisySetRuntimeFetch;
#pragma warning restore CS0649
    }
}
