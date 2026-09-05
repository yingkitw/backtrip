using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Threading;

namespace RoundtripDemo;

public class Engine<T> : IWorker
{
    private T _state;

    public string Name { get; set; }
    public T State { get; }

    public Engine(T initial, string name)
    {
        this.State = initial;
        this.Name = name;
        this._state = initial;
        return;    }

    public U Map<U>(Func<T, U> f)
    {
        return f.Invoke(this._state);    }

    public virtual int DoWork(int units)
    {
        return units;    }

}
