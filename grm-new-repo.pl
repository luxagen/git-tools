#!/usr/bin/perl

use strict;
use warnings;
use IPC::Run qw(run);

# Helper function to run git commands
sub run_git {
    my ($capture_output, @args) = @_;

    if ($capture_output) {
        my $out = '';
        my $err = '';
        my $success = run(['git', @args], '>', \$out, '2>', \$err);
        return ($success, $out, $err);
    } else {
        return run(['git', @args]);
    }
}

# Check required environment variables
my @missing;
for my $var (qw(GRM_CFGCMD GRM_RPATH_TEMPLATE GRM_RLOGIN)) {
    if (!defined $ENV{$var}) {
        push @missing, $var;
    }
}

if (@missing) {
    print STDERR "The following environment variables must be set: ", join(" ", @missing), "\n";
    exit 2;
}

# Determine whether this dir is already a git repo
my $virgin = !(-d ".git");

# Set path base
my $grm_rpath_base = '/git/music-projects';

# Get local and remote relative paths
my ($lrel, $rrel);
run(['grm', 'list-lrel'], '>', \$lrel);
chomp($lrel);
run(['grm', 'list-rrel'], '>', \$rrel);
chomp($rrel);
my $grm_rpath = "$grm_rpath_base/$rrel";
$grm_rpath .= ".git" unless $grm_rpath =~ /\.git$/;

# There must be exactly one GRM-known (sub)directory and it must be .
if ($rrel eq "" || $ENV{PWD} !~ /$lrel$/) {
    print "The current directory is unknown to GRM!\n";
    exit 1;
}

print "About to create remote repo '$grm_rpath'; are you sure? ";
my $reply = <STDIN>;
chomp($reply);
if ($reply !~ /^[Yy]$/) {
    print "(aborted)\n";
    exit 0;
}

# Initialize git repository
&run_git(0, 'init', '-q');

# Run the config command
my @cfg_cmd = split /\s+/, $ENV{GRM_CFGCMD}; # TODO: remove split
run(\@cfg_cmd);

# Set error handling mode (no direct equivalent in IPC::Run, handled by checking return values)

# Create new remote repo based on remote template
my $ssh_cmd = "cp -na --reflink=auto '$grm_rpath_base/$ENV{GRM_RPATH_TEMPLATE}' '$grm_rpath'";
run(['ssh', $ENV{GRM_RLOGIN}, $ssh_cmd]);

my $ssh_rpath = "ssh://$ENV{GRM_RLOGIN}$grm_rpath";

# Check if remote exists, add or update it accordingly
my ($remote_exists, $out, $err) = &run_git(1, 'remote', 'get-url', 'origin');

if ($remote_exists) {
    # Remote exists, update it
    &run_git(0, 'remote', 'set-url', 'origin', $ssh_rpath);
    &run_git(0, 'fetch', 'origin');
} else {
    # Remote doesn't exist, add it
    &run_git(0, 'remote', 'add', '-f', 'origin', $ssh_rpath);
}

if ($virgin) {
    &run_git(0, 'checkout', 'master');
}