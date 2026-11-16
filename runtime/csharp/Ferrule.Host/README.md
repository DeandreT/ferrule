# Ferrule.Host

This package includes the .NET 10 bindings and the glibc-based Linux x64 native library. Ferrule builds and tests the package on Ubuntu 24.04. A framework-dependent app can reference `Ferrule.Host` and run with a .NET 10 installation. For RID-specific publishing, use `linux-x64`.

```csharp
using Ferrule.Host;

using var project = FerruleProject.Load(File.ReadAllBytes("mapping.json"));
InspectionReport inspection = project.Inspect();
ValidationReport validation = project.Validate();
```

## License and corresponding source

Ferrule.Host is licensed under GPL-3.0-only. The package includes the license text and records its Git repository URL and full source commit in the package manifest. Browse the exact source at that repository’s `/tree/<commit>` URL or download the corresponding source archive, including build scripts, from its `/archive/<commit>.tar.gz` URL.
