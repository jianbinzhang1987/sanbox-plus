using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System.Collections.ObjectModel;

namespace SandboxShell.WinUI;

public sealed partial class MainWindow : Window
{
    private readonly SandboxServiceClient service = new();
    private readonly ObservableCollection<SandboxAppViewModel> apps = new();
    private SandboxInstanceViewModel? activeInstance;

    public MainWindow()
    {
        InitializeComponent();
        AppsList.ItemsSource = apps;
        _ = RefreshAsync();
    }

    private async void RefreshButton_Click(object sender, RoutedEventArgs e) => await RefreshAsync();

    private async void LaunchButton_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as Button)?.DataContext is SandboxAppViewModel app && activeInstance is not null)
        {
            await service.LaunchAppAsync(activeInstance, app);
        }
    }

    private async void ReturnButton_Click(object sender, RoutedEventArgs e)
    {
        if (activeInstance is null)
        {
            return;
        }

        try
        {
            await service.ReturnToHostAsync(activeInstance);
        }
        catch (Exception error)
        {
            StatusText.Text = error.Message;
        }
    }

    private async Task RefreshAsync()
    {
        apps.Clear();
        try
        {
            var status = await service.GetStatusAsync();
            activeInstance = status.Instance;
            if (activeInstance is null)
            {
                StatusText.Text = "No active session";
                return;
            }

            var serviceApps = await service.ListAppsAsync(activeInstance.Id);
            foreach (var app in serviceApps)
            {
                apps.Add(app);
            }
            StatusText.Text = serviceApps.Count == 0 ? "No policy apps" : $"{serviceApps.Count} policy app(s)";
        }
        catch (Exception error)
        {
            activeInstance = null;
            StatusText.Text = error.Message;
        }
    }

}
