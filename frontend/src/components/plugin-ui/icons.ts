/** Icon names plugins may use (kept out of PluginRenderer so the app shell
 * does not pull the renderer and its chart library into the main bundle). */
import * as P from "@phosphor-icons/react";

export const ICONS: Record<string, P.Icon> = {
  HardDrives: P.HardDrivesIcon, ComputerTower: P.ComputerTowerIcon, Cloud: P.CloudIcon, Cpu: P.CpuIcon, Database: P.DatabaseIcon,
  Globe: P.GlobeIcon, Gauge: P.GaugeIcon, ChartBar: P.ChartBarIcon, ChartLine: P.ChartLineIcon, Lightning: P.LightningIcon,
  Power: P.PowerIcon, Play: P.PlayIcon, Pause: P.PauseIcon, Stop: P.StopIcon, ArrowsClockwise: P.ArrowsClockwiseIcon,
  Check: P.CheckIcon, CheckCircle: P.CheckCircleIcon, Warning: P.WarningIcon, WarningCircle: P.WarningCircleIcon, Info: P.InfoIcon,
  X: P.XIcon, Plus: P.PlusIcon, Trash: P.TrashIcon, PencilSimple: P.PencilSimpleIcon, Gear: P.GearIcon, Key: P.KeyIcon,
  Lock: P.LockIcon, User: P.UserIcon, Users: P.UsersIcon, Envelope: P.EnvelopeIcon, Bell: P.BellIcon, Terminal: P.TerminalIcon,
  Code: P.CodeIcon, Download: P.DownloadSimpleIcon, Upload: P.UploadSimpleIcon, Link: P.LinkIcon, PuzzlePiece: P.PuzzlePieceIcon,
  Package: P.PackageIcon, CreditCard: P.CreditCardIcon, Receipt: P.ReceiptIcon, Ticket: P.TicketIcon, ShieldCheck: P.ShieldCheckIcon,
};
