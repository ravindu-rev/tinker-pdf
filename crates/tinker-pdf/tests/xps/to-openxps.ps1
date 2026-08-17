# Rewrites an XPS package as OpenXPS through Windows' own XPS Document API.
#
# The route gap 30's plan names for OpenXPS is the "Microsoft XPS Document
# Writer" printer, which needs the Printing-XPSServices-Features optional
# feature; that feature is not installed here and enabling it needs elevation
# this session does not have. But the printer is not the only Microsoft
# component that writes the dialect: the XPS Document API's object model --
# the "XPS Object Factory" coclass, {E974D26D-3D9B-4D47-88CC-3872F2DC3585},
# served by %WINDIR%\System32\XpsServices.dll -- is registered on a stock
# Windows 11 install without that feature, and IXpsOMPackage1::WriteToFile1
# takes an XPS_DOCUMENT_TYPE. Passing XPS_DOCUMENT_TYPE_OPENXPS makes Microsoft
# code, not this repository, write every byte of the result.
#
# Vtable layouts below are the Windows SDK's, from xpsobjectmodel.h and
# xpsobjectmodel_1.h. Methods ahead of the ones actually called are declared as
# placeholders so the slots line up, and are never invoked. Getting one of them
# wrong would not be subtle: the call would land in the wrong slot.

param(
    [Parameter(Mandatory = $true)][string]$In,
    [Parameter(Mandatory = $true)][string]$Out,
    [ValidateSet('xps', 'openxps')][string]$Type = 'openxps'
)
$ErrorActionPreference = 'Stop'

Add-Type -Language CSharp @'
using System;
using System.Runtime.InteropServices;

namespace XpsOm {

[ComImport, Guid("f9b2a685-a50d-4fc2-b764-b56e093ea0ca"),
 InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IXpsOMObjectFactory
{
    [PreserveSig] int CreatePackage(out IntPtr package);
    [PreserveSig] int CreatePackageFromFile(
        [MarshalAs(UnmanagedType.LPWStr)] string filename,
        [MarshalAs(UnmanagedType.Bool)] bool reuseObjects,
        [MarshalAs(UnmanagedType.IUnknown)] out object package);
    // the remaining ~40 methods are deliberately undeclared and never called
}

[ComImport, Guid("95a9435e-12bb-461b-8e7f-c6adb04cd96a"),
 InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IXpsOMPackage1
{
    // IXpsOMPackage, ten methods, in vtable order
    [PreserveSig] int GetDocumentSequence(out IntPtr p);
    [PreserveSig] int SetDocumentSequence(IntPtr p);
    [PreserveSig] int GetCoreProperties(out IntPtr p);
    [PreserveSig] int SetCoreProperties(IntPtr p);
    [PreserveSig] int GetDiscardControlPartName(out IntPtr p);
    [PreserveSig] int SetDiscardControlPartName(IntPtr p);
    [PreserveSig] int GetThumbnailResource(out IntPtr p);
    [PreserveSig] int SetThumbnailResource(IntPtr p);
    [PreserveSig] int WriteToFile([MarshalAs(UnmanagedType.LPWStr)] string f, IntPtr sa,
        uint attrs, [MarshalAs(UnmanagedType.Bool)] bool optimize);
    [PreserveSig] int WriteToStream(IntPtr s, [MarshalAs(UnmanagedType.Bool)] bool optimize);
    // IXpsOMPackage1
    [PreserveSig] int GetDocumentType(out int documentType);
    [PreserveSig] int WriteToFile1([MarshalAs(UnmanagedType.LPWStr)] string fileName, IntPtr sa,
        uint flagsAndAttributes, [MarshalAs(UnmanagedType.Bool)] bool optimizeMarkupSize,
        int documentType);
    [PreserveSig] int WriteToStream1(IntPtr outputStream,
        [MarshalAs(UnmanagedType.Bool)] bool optimizeMarkupSize, int documentType);
}

public static class Rewriter
{
    // XPS_DOCUMENT_TYPE: UNSPECIFIED = 1, XPS = 2, OPENXPS = 3
    const uint FILE_ATTRIBUTE_NORMAL = 0x80;

    public static int Run(string inPath, string outPath, int docType)
    {
        Type t = Type.GetTypeFromCLSID(new Guid("E974D26D-3D9B-4D47-88CC-3872F2DC3585"));
        object raw = Activator.CreateInstance(t);
        var factory = (IXpsOMObjectFactory)raw;
        object pkgObj;
        int hr = factory.CreatePackageFromFile(inPath, false, out pkgObj);
        if (hr < 0) Marshal.ThrowExceptionForHR(hr);
        var pkg = (IXpsOMPackage1)pkgObj;

        int sourceType;
        hr = pkg.GetDocumentType(out sourceType);
        if (hr < 0) Marshal.ThrowExceptionForHR(hr);

        hr = pkg.WriteToFile1(outPath, IntPtr.Zero, FILE_ATTRIBUTE_NORMAL, false, docType);
        if (hr < 0) Marshal.ThrowExceptionForHR(hr);

        Marshal.ReleaseComObject(pkgObj);
        Marshal.ReleaseComObject(raw);
        return sourceType;
    }
}
}
'@

if (Test-Path $Out) { Remove-Item $Out -Force }
$code = if ($Type -eq 'openxps') { 3 } else { 2 }
$sourceType = [XpsOm.Rewriter]::Run((Resolve-Path $In).Path, $Out, $code)
"wrote {0,-30} {1,8} bytes  (source document type {2}, 2=xps 3=openxps)" -f `
    (Split-Path $Out -Leaf), (Get-Item $Out).Length, $sourceType
