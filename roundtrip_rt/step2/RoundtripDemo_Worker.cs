using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Threading;

namespace RoundtripDemo;

public class Worker : IWorker, IReset
{
    public const int MaxUnits = 999;
    public static int Built;
    private static int TotalDone;
    private object _sync;
    public int Done;

    public string Name { get; set; }

    public event Compute Progress;

    public Worker(string name)
    {
        this._sync = new object();
        this.Name = name;
        this.Done = 0;
        RoundtripDemo.Worker.Built = RoundtripDemo.Worker.Built + 1;
        return;    }

    static Worker()
    {
        RoundtripDemo.Worker.Built = 0;
        RoundtripDemo.Worker.TotalDone = 0;
        return;    }

    public virtual int DoWork(int units)
    {
        this.Done = this.Done + units;
        RoundtripDemo.Worker.TotalDone = RoundtripDemo.Worker.TotalDone + units;
        return this.Done;    }

     void IReset.Reset()
    {
        this.Done = 0;
        return;    }

    public void Bump()
    {

        lock (this._sync) {
            this.Done = this.Done + 1;
        }
        return;    }

}
